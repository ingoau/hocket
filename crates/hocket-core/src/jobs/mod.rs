//! Job queue: durable, bounded concurrency, resumable, cancellable, pausable;
//! problems list. Owner: core-server.
//!
//! Entry points:
//! - [`JobQueue::new`] then [`JobQueue::register`] a [`JobRunner`] per
//!   [`JobKind`] with its concurrency (downloads 2, ratings 4, ...), then
//!   [`JobQueue::recover`] once on start (jobs that were running when the
//!   process died go back to queued) and [`JobQueue::start`] the scheduler on
//!   the runtime. Tests drive it with [`JobQueue::run_until_idle`].
//! - [`JobQueue::submit`] a [`JobSpec`] (kind, label, opaque payload JSON and
//!   optional per-item list persisted in `job_items`) → [`JobId`].
//! - [`JobQueue::cancel`] / [`pause`](JobQueue::pause) / [`resume`](JobQueue::resume) /
//!   [`retry`](JobQueue::retry); [`JobQueue::jobs`] → `Vec<api::Job>` and
//!   [`JobQueue::problems`] → `Vec<api::Problem>`; [`JobQueue::on_change`] to
//!   receive [`QueueEvent`]s for `Event::JobsChanged` / `Event::ProblemsChanged`.
//! - A runner gets a [`JobContext`]: pending items, progress, cursor
//!   checkpoints (crash resume), `checkpoint().await` for pause/cancel.
//! - Failures never interrupt: a job that ends with failed items is
//!   `JobState::Failed` ("finished with problems") and one retryable
//!   [`api::Problem`] is filed. [`classify_failure`] says whether a failure
//!   deserves a toast (only an immediate failure of a user action does).

use std::collections::HashMap;
use std::sync::Arc;

use futures::future::BoxFuture;
use parking_lot::{Mutex, RwLock};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use tokio::sync::{watch, Notify, Semaphore};
use tokio_util::sync::CancellationToken;

use crate::api::{Job, JobId, JobKind, JobState, Problem};
use crate::db::{Db, DbError, DbResult};
use crate::util::{new_id, Clock};

#[derive(Debug, thiserror::Error)]
pub enum JobError {
    #[error("cancelled")]
    Cancelled,
    /// The whole job failed (as opposed to individual items). Filed as a problem.
    #[error("{0}")]
    Failed(String),
    #[error("db: {0}")]
    Db(#[from] DbError),
    #[error("no runner registered for {0:?}")]
    NoRunner(JobKind),
    #[error("unknown job {0}")]
    UnknownJob(String),
}

pub type JobResult<T> = Result<T, JobError>;

/// What a job is asked to do.
#[derive(Debug, Clone)]
pub struct JobSpec {
    pub kind: JobKind,
    pub label: String,
    /// Opaque JSON for the runner (e.g. `{"full":true}`).
    pub payload: String,
    /// Per-item work, persisted; `None` total when the runner discovers items itself.
    pub items: Vec<String>,
    pub cancellable: bool,
}

impl JobSpec {
    pub fn new(kind: JobKind, label: impl Into<String>) -> Self {
        JobSpec {
            kind,
            label: label.into(),
            payload: "{}".into(),
            items: vec![],
            cancellable: true,
        }
    }
    pub fn payload(mut self, json: impl Into<String>) -> Self {
        self.payload = json.into();
        self
    }
    pub fn items(mut self, items: Vec<String>) -> Self {
        self.items = items;
        self
    }
    pub fn not_cancellable(mut self) -> Self {
        self.cancellable = false;
        self
    }
}

/// One persisted item of a job.
#[derive(Debug, Clone, PartialEq)]
pub struct JobItem {
    pub seq: i64,
    pub item: String,
    pub attempts: u32,
}

/// Emitted to listeners whenever the visible state changes.
#[derive(Debug, Clone)]
pub enum QueueEvent {
    JobsChanged(Vec<Job>),
    ProblemsChanged(Vec<Problem>),
}

/// Where a failure should surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureSurface {
    /// A toast: the user just did this and it failed right away.
    Toast,
    /// The problems list behind the job indicator; nothing interrupts.
    ProblemsList,
}

/// The only toast-worthy failure is an immediate one on a user action;
/// everything asynchronous (jobs, outbox retries, sync) goes to the list.
pub fn classify_failure(immediate_user_action: bool) -> FailureSurface {
    if immediate_user_action {
        FailureSurface::Toast
    } else {
        FailureSurface::ProblemsList
    }
}

/// Runs jobs of one kind. Must honour `ctx.checkpoint().await` regularly.
pub trait JobRunner: Send + Sync + 'static {
    fn run(&self, ctx: JobContext) -> BoxFuture<'static, JobResult<()>>;
}

impl<F> JobRunner for F
where
    F: Fn(JobContext) -> BoxFuture<'static, JobResult<()>> + Send + Sync + 'static,
{
    fn run(&self, ctx: JobContext) -> BoxFuture<'static, JobResult<()>> {
        self(ctx)
    }
}

struct Registered {
    runner: Arc<dyn JobRunner>,
    semaphore: Arc<Semaphore>,
}

struct Running {
    cancel: CancellationToken,
    pause: watch::Sender<bool>,
}

/// Retry payload stored on a problem.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "type", content = "data")]
pub enum RetryAction {
    /// Re-run the failed items of the job (or the whole job if it had none).
    RetryJob { job_id: String },
    /// Re-submit a fresh job with this spec (used by the outbox and downloads).
    Resubmit {
        kind: JobKind,
        label: String,
        payload: String,
        items: Vec<String>,
    },
}

type Listener = Box<dyn Fn(QueueEvent) + Send + Sync>;

struct Inner {
    db: Db,
    clock: Arc<dyn Clock>,
    runners: RwLock<HashMap<&'static str, Registered>>,
    running: Mutex<HashMap<JobId, Running>>,
    listeners: Mutex<Vec<Listener>>,
    notify: Notify,
    /// Signalled whenever a running job finishes (for `run_until_idle`).
    finished: Notify,
    /// Terminal jobs older than this are pruned from the list (ms).
    retention_ms: f64,
}

#[derive(Clone)]
pub struct JobQueue {
    inner: Arc<Inner>,
}

const KIND_NAMES: &[(JobKind, &str)] = &[
    (JobKind::LibrarySync, "librarySync"),
    (JobKind::BulkRating, "bulkRating"),
    (JobKind::BulkLove, "bulkLove"),
    (JobKind::Download, "download"),
    (JobKind::PlaylistImport, "playlistImport"),
    (JobKind::OutboxFlush, "outboxFlush"),
    (JobKind::ArtworkPrefetch, "artworkPrefetch"),
    (JobKind::LyricsPrefetch, "lyricsPrefetch"),
    (JobKind::FilterMaterialise, "filterMaterialise"),
];

pub fn kind_name(k: JobKind) -> &'static str {
    KIND_NAMES
        .iter()
        .find(|(kk, _)| *kk == k)
        .map(|(_, n)| *n)
        .unwrap_or("unknown")
}

pub fn kind_from_name(s: &str) -> Option<JobKind> {
    KIND_NAMES.iter().find(|(_, n)| *n == s).map(|(k, _)| *k)
}

/// Persisted name of a job state.
pub fn state_name(s: JobState) -> &'static str {
    match s {
        JobState::Queued => "queued",
        JobState::Running => "running",
        JobState::Paused => "paused",
        JobState::Done => "done",
        JobState::Failed => "failed",
        JobState::Cancelled => "cancelled",
    }
}

fn state_from_name(s: &str) -> JobState {
    match s {
        "queued" => JobState::Queued,
        "running" => JobState::Running,
        "paused" => JobState::Paused,
        "done" => JobState::Done,
        "failed" => JobState::Failed,
        _ => JobState::Cancelled,
    }
}

const JOB_COLUMNS: &str = "id, kind, label, state, done, total, failed, created_at, cancellable";

fn job_from_row(r: &rusqlite::Row) -> rusqlite::Result<Job> {
    let kind: String = r.get(1)?;
    let state: String = r.get(3)?;
    Ok(Job {
        id: r.get(0)?,
        kind: kind_from_name(&kind).unwrap_or(JobKind::OutboxFlush),
        label: r.get(2)?,
        state: state_from_name(&state),
        done: r.get::<_, i64>(4)?.max(0) as u32,
        total: r.get::<_, Option<i64>>(5)?.map(|v| v.max(0) as u32),
        failed: r.get::<_, i64>(6)?.max(0) as u32,
        created_at: r.get(7)?,
        cancellable: r.get::<_, i64>(8)? != 0,
    })
}

impl JobQueue {
    pub fn new(db: Db, clock: Arc<dyn Clock>) -> Self {
        JobQueue {
            inner: Arc::new(Inner {
                db,
                clock,
                runners: RwLock::new(HashMap::new()),
                running: Mutex::new(HashMap::new()),
                listeners: Mutex::new(Vec::new()),
                notify: Notify::new(),
                finished: Notify::new(),
                retention_ms: 24.0 * 3600.0 * 1000.0,
            }),
        }
    }

    pub fn db(&self) -> &Db {
        &self.inner.db
    }

    pub fn clock(&self) -> &Arc<dyn Clock> {
        &self.inner.clock
    }

    /// Register the runner for a kind with its maximum parallelism.
    pub fn register(&self, kind: JobKind, concurrency: usize, runner: Arc<dyn JobRunner>) {
        self.inner.runners.write().insert(
            kind_name(kind),
            Registered {
                runner,
                semaphore: Arc::new(Semaphore::new(concurrency.max(1))),
            },
        );
        self.inner.notify.notify_one();
    }

    pub fn on_change(&self, f: impl Fn(QueueEvent) + Send + Sync + 'static) {
        self.inner.listeners.lock().push(Box::new(f));
    }

    fn emit_jobs(&self) {
        if let Ok(jobs) = self.jobs() {
            let ls = self.inner.listeners.lock();
            for l in ls.iter() {
                l(QueueEvent::JobsChanged(jobs.clone()));
            }
        }
    }

    fn emit_problems(&self) {
        if let Ok(p) = self.problems() {
            let ls = self.inner.listeners.lock();
            for l in ls.iter() {
                l(QueueEvent::ProblemsChanged(p.clone()));
            }
        }
    }

    /// Reset jobs left `running` by a previous process to `queued` so they
    /// resume from their cursor. Call once before `start`.
    pub fn recover(&self) -> DbResult<usize> {
        let n = self.inner.db.with_conn(|c| {
            Ok(c.execute(
                "UPDATE jobs SET state = 'queued', updated_at = ?1 WHERE state = 'running'",
                [self.inner.clock.now_ms()],
            )?)
        })?;
        self.inner.notify.notify_one();
        Ok(n)
    }

    pub fn submit(&self, spec: JobSpec) -> DbResult<JobId> {
        let id = new_id();
        let now = self.inner.clock.now_ms();
        let total: Option<i64> = if spec.items.is_empty() {
            None
        } else {
            Some(spec.items.len() as i64)
        };
        self.inner.db.with_tx(|tx| {
            tx.execute(
                "INSERT INTO jobs(id, kind, label, state, payload, done, total, failed, created_at, updated_at, cancellable) VALUES (?1, ?2, ?3, 'queued', ?4, 0, ?5, 0, ?6, ?6, ?7)",
                params![id, kind_name(spec.kind), spec.label, spec.payload, total, now, spec.cancellable as i64],
            )?;
            let mut st = tx.prepare_cached("INSERT INTO job_items(job_id, seq, item, state) VALUES (?1, ?2, ?3, 'pending')")?;
            for (i, item) in spec.items.iter().enumerate() {
                st.execute(params![id, i as i64, item])?;
            }
            Ok(())
        })?;
        self.inner.notify.notify_one();
        self.emit_jobs();
        Ok(id)
    }

    /// Visible jobs: everything non-terminal plus terminal ones within retention, newest first.
    pub fn jobs(&self) -> DbResult<Vec<Job>> {
        let cutoff = self.inner.clock.now_ms() - self.inner.retention_ms;
        self.inner.db.with_conn(|c| {
            let mut st = c.prepare_cached(&format!(
                "SELECT {JOB_COLUMNS} FROM jobs WHERE state IN ('queued','running','paused') OR updated_at >= ?1 ORDER BY created_at DESC"
            ))?;
            let rows = st.query_map([cutoff], job_from_row)?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
    }

    pub fn job(&self, id: &str) -> DbResult<Option<Job>> {
        self.inner.db.with_conn(|c| {
            Ok(c.query_row(
                &format!("SELECT {JOB_COLUMNS} FROM jobs WHERE id = ?1"),
                [id],
                job_from_row,
            )
            .optional()?)
        })
    }

    pub fn payload(&self, id: &str) -> DbResult<Option<String>> {
        self.inner.db.with_conn(|c| {
            Ok(
                c.query_row("SELECT payload FROM jobs WHERE id = ?1", [id], |r| r.get(0))
                    .optional()?,
            )
        })
    }

    /// Cancel a queued/running/paused job. Running jobs stop at their next checkpoint.
    pub fn cancel(&self, id: &str) -> DbResult<bool> {
        let now = self.inner.clock.now_ms();
        let changed = self.inner.db.with_tx(|tx| {
            let n = tx.execute(
                "UPDATE jobs SET state = 'cancelled', updated_at = ?2 WHERE id = ?1 AND cancellable = 1 AND state IN ('queued','running','paused')",
                params![id, now],
            )?;
            if n > 0 {
                tx.execute("UPDATE job_items SET state = 'cancelled' WHERE job_id = ?1 AND state = 'pending'", [id])?;
            }
            Ok(n > 0)
        })?;
        if changed {
            if let Some(r) = self.inner.running.lock().get(id) {
                r.cancel.cancel();
                let _ = r.pause.send(false);
            }
            self.emit_jobs();
        }
        Ok(changed)
    }

    pub fn pause(&self, id: &str) -> DbResult<bool> {
        let now = self.inner.clock.now_ms();
        let changed = self.inner.db.with_conn(|c| {
            Ok(c.execute("UPDATE jobs SET state = 'paused', updated_at = ?2 WHERE id = ?1 AND state IN ('queued','running')", params![id, now])? > 0)
        })?;
        if changed {
            if let Some(r) = self.inner.running.lock().get(id) {
                let _ = r.pause.send(true);
            }
            self.emit_jobs();
        }
        Ok(changed)
    }

    pub fn resume(&self, id: &str) -> DbResult<bool> {
        let now = self.inner.clock.now_ms();
        let is_running = self.inner.running.lock().contains_key(id);
        let new_state = if is_running { "running" } else { "queued" };
        let changed = self.inner.db.with_conn(|c| {
            Ok(c.execute(
                "UPDATE jobs SET state = ?3, updated_at = ?2 WHERE id = ?1 AND state = 'paused'",
                params![id, now, new_state],
            )? > 0)
        })?;
        if changed {
            if let Some(r) = self.inner.running.lock().get(id) {
                let _ = r.pause.send(false);
            }
            self.inner.notify.notify_one();
            self.emit_jobs();
        }
        Ok(changed)
    }

    /// Re-queue a finished job: failed items go back to pending (a job without
    /// items simply runs again from its cursor).
    pub fn retry(&self, id: &str) -> DbResult<bool> {
        let now = self.inner.clock.now_ms();
        let changed = self.inner.db.with_tx(|tx| {
            let n = tx.execute(
                "UPDATE jobs SET state = 'queued', failed = 0, updated_at = ?2 WHERE id = ?1 AND state IN ('failed','cancelled','done')",
                params![id, now],
            )?;
            if n > 0 {
                tx.execute("UPDATE job_items SET state = 'pending', error = NULL WHERE job_id = ?1 AND state IN ('failed','cancelled')", [id])?;
                tx.execute("UPDATE jobs SET done = (SELECT count(*) FROM job_items WHERE job_id = ?1 AND state = 'done') WHERE id = ?1", [id])?;
            }
            Ok(n > 0)
        })?;
        if changed {
            self.inner.notify.notify_one();
            self.emit_jobs();
        }
        Ok(changed)
    }

    /// Whether a job of this kind is queued/running/paused (e.g. avoid two syncs).
    pub fn has_active(&self, kind: JobKind) -> DbResult<bool> {
        self.inner.db.with_conn(|c| {
            let n: i64 = c.query_row(
                "SELECT count(*) FROM jobs WHERE kind = ?1 AND state IN ('queued','running','paused')",
                [kind_name(kind)],
                |r| r.get(0),
            )?;
            Ok(n > 0)
        })
    }

    // -- problems -----------------------------------------------------------

    pub fn add_problem(
        &self,
        job_id: Option<&str>,
        summary: &str,
        detail: Option<&str>,
        retry: Option<RetryAction>,
    ) -> DbResult<String> {
        let id = new_id();
        let now = self.inner.clock.now_ms();
        let retry_json = match &retry {
            Some(r) => Some(serde_json::to_string(r)?),
            None => None,
        };
        self.inner.db.with_conn(|c| {
            c.execute(
                "INSERT INTO problems(id, job_id, summary, detail, created_at, retryable, retry, dismissed) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0)",
                params![id, job_id, summary, detail, now, retry.is_some() as i64, retry_json],
            )?;
            Ok(())
        })?;
        self.emit_problems();
        Ok(id)
    }

    pub fn problems(&self) -> DbResult<Vec<Problem>> {
        self.inner.db.with_conn(|c| {
            let mut st = c.prepare_cached(
                "SELECT id, job_id, summary, detail, created_at, retryable FROM problems WHERE dismissed = 0 ORDER BY created_at DESC",
            )?;
            let rows = st.query_map([], |r| {
                Ok(Problem {
                    id: r.get(0)?,
                    job_id: r.get(1)?,
                    summary: r.get(2)?,
                    detail: r.get(3)?,
                    created_at: r.get(4)?,
                    retryable: r.get::<_, i64>(5)? != 0,
                })
            })?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
    }

    pub fn dismiss_problem(&self, id: &str) -> DbResult<bool> {
        let n = self.inner.db.with_conn(|c| {
            Ok(c.execute("UPDATE problems SET dismissed = 1 WHERE id = ?1", [id])?)
        })?;
        if n > 0 {
            self.emit_problems();
        }
        Ok(n > 0)
    }

    pub fn dismiss_all_problems(&self) -> DbResult<usize> {
        let n = self.inner.db.with_conn(|c| {
            Ok(c.execute("UPDATE problems SET dismissed = 1 WHERE dismissed = 0", [])?)
        })?;
        self.emit_problems();
        Ok(n)
    }

    /// Run a problem's retry action and dismiss it. Returns the job id the
    /// retry runs under, if any.
    pub fn retry_problem(&self, id: &str) -> DbResult<Option<JobId>> {
        let retry: Option<String> = self.inner.db.with_conn(|c| {
            Ok(c.query_row(
                "SELECT retry FROM problems WHERE id = ?1 AND dismissed = 0",
                [id],
                |r| r.get(0),
            )
            .optional()?
            .flatten())
        })?;
        let Some(retry) = retry else { return Ok(None) };
        let action: RetryAction = serde_json::from_str(&retry)?;
        let job_id = match action {
            RetryAction::RetryJob { job_id } => {
                if self.retry(&job_id)? {
                    Some(job_id)
                } else {
                    None
                }
            }
            RetryAction::Resubmit {
                kind,
                label,
                payload,
                items,
            } => Some(self.submit(JobSpec {
                kind,
                label,
                payload,
                items,
                cancellable: true,
            })?),
        };
        self.dismiss_problem(id)?;
        Ok(job_id)
    }

    // -- scheduling ---------------------------------------------------------

    /// Spawn the scheduler loop on the runtime. Returns immediately.
    pub fn start(&self, handle: &tokio::runtime::Handle) {
        let q = self.clone();
        handle.spawn(async move {
            loop {
                if let Err(e) = q.schedule_once() {
                    tracing::error!(error = %e, "job scheduler");
                }
                tokio::select! {
                    _ = q.inner.notify.notified() => {}
                    _ = tokio::time::sleep(std::time::Duration::from_secs(2)) => {}
                }
            }
        });
    }

    /// Drive the queue until nothing is queued or running (tests and the
    /// simulation harness). Paused jobs count as idle.
    pub async fn run_until_idle(&self) -> DbResult<()> {
        loop {
            self.schedule_once()?;
            let running = self.inner.running.lock().len();
            let queued_runnable: i64 = self.inner.db.with_conn(|c| {
                let kinds: Vec<String> = self
                    .inner
                    .runners
                    .read()
                    .keys()
                    .map(|k| k.to_string())
                    .collect();
                let mut n = 0;
                for k in kinds {
                    n += c.query_row(
                        "SELECT count(*) FROM jobs WHERE state = 'queued' AND kind = ?1",
                        [k],
                        |r| r.get::<_, i64>(0),
                    )?;
                }
                Ok(n)
            })?;
            if running == 0 && queued_runnable == 0 {
                return Ok(());
            }
            if running == 0 {
                // queued but no capacity? shouldn't happen; avoid a hot loop
                tokio::task::yield_now().await;
                continue;
            }
            tokio::select! {
                _ = self.inner.finished.notified() => {}
                _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {}
            }
        }
    }

    /// Start every queued job whose kind has a free slot.
    pub fn schedule_once(&self) -> DbResult<()> {
        let queued: Vec<(String, String)> = self.inner.db.with_conn(|c| {
            let mut st = c.prepare_cached(
                "SELECT id, kind FROM jobs WHERE state = 'queued' ORDER BY created_at ASC",
            )?;
            let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })?;
        for (id, kind_s) in queued {
            let Some(kind) = kind_from_name(&kind_s) else {
                continue;
            };
            if self.inner.running.lock().contains_key(&id) {
                continue;
            }
            let (runner, permit) = {
                let runners = self.inner.runners.read();
                let Some(reg) = runners.get(kind_name(kind)) else {
                    continue;
                };
                let Ok(permit) = reg.semaphore.clone().try_acquire_owned() else {
                    continue;
                };
                (reg.runner.clone(), permit)
            };
            self.launch(id, kind, runner, permit)?;
        }
        Ok(())
    }

    fn launch(
        &self,
        id: String,
        kind: JobKind,
        runner: Arc<dyn JobRunner>,
        permit: tokio::sync::OwnedSemaphorePermit,
    ) -> DbResult<()> {
        let now = self.inner.clock.now_ms();
        let payload = self.payload(&id)?.unwrap_or_else(|| "{}".into());
        self.inner.db.with_conn(|c| {
            c.execute(
                "UPDATE jobs SET state = 'running', updated_at = ?2 WHERE id = ?1",
                params![id, now],
            )?;
            Ok(())
        })?;
        let cancel = CancellationToken::new();
        let (pause_tx, pause_rx) = watch::channel(false);
        self.inner.running.lock().insert(
            id.clone(),
            Running {
                cancel: cancel.clone(),
                pause: pause_tx,
            },
        );
        self.emit_jobs();
        let ctx = JobContext {
            queue: self.clone(),
            job_id: id.clone(),
            kind,
            payload,
            cancel,
            pause: pause_rx,
        };
        let q = self.clone();
        let fut = runner.run(ctx);
        tokio::spawn(async move {
            let result = fut.await;
            drop(permit);
            q.finish(&id, result);
        });
        Ok(())
    }

    fn finish(&self, id: &str, result: JobResult<()>) {
        self.inner.running.lock().remove(id);
        let now = self.inner.clock.now_ms();
        let outcome = self.inner.db.with_tx(|tx| {
            let (state, label, failed): (String, String, i64) = tx.query_row(
                "SELECT state, label, failed FROM jobs WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            let cancelled = state == "cancelled" || matches!(result, Err(JobError::Cancelled));
            let item_failed: i64 =
                tx.query_row("SELECT count(*) FROM job_items WHERE job_id = ?1 AND state = 'failed'", [id], |r| r.get(0))?;
            let failed = failed.max(item_failed);
            let new_state = if cancelled {
                "cancelled"
            } else if result.is_err() || failed > 0 {
                "failed"
            } else {
                "done"
            };
            tx.execute("UPDATE jobs SET state = ?2, failed = ?3, updated_at = ?4 WHERE id = ?1", params![id, new_state, failed, now])?;
            let errors: Vec<String> = {
                let mut st = tx.prepare_cached("SELECT item, error FROM job_items WHERE job_id = ?1 AND state = 'failed' ORDER BY seq LIMIT 5")?;
                let rows = st.query_map([id], |r| Ok(format!("{}: {}", r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?.unwrap_or_default())))?;
                rows.collect::<Result<Vec<_>, _>>()?
            };
            Ok((new_state.to_string(), label, failed, errors))
        });
        match outcome {
            Ok((state, label, failed, errors)) => {
                if state == "failed" {
                    let (summary, detail) = match &result {
                        Err(JobError::Failed(m)) => (format!("{label} failed"), Some(m.clone())),
                        Err(JobError::Db(e)) => (format!("{label} failed"), Some(e.to_string())),
                        _ => (
                            format!(
                                "{label}: {failed} item{} failed",
                                if failed == 1 { "" } else { "s" }
                            ),
                            if errors.is_empty() {
                                None
                            } else {
                                Some(errors.join("\n"))
                            },
                        ),
                    };
                    if let Err(e) = self.add_problem(
                        Some(id),
                        &summary,
                        detail.as_deref(),
                        Some(RetryAction::RetryJob {
                            job_id: id.to_string(),
                        }),
                    ) {
                        tracing::error!(error = %e, "filing problem");
                    }
                }
                tracing::info!(job = id, state, failed, "job finished");
            }
            Err(e) => tracing::error!(error = %e, job = id, "finishing job"),
        }
        self.emit_jobs();
        self.inner.finished.notify_waiters();
        self.inner.notify.notify_one();
    }

    /// Delete terminal jobs older than retention (housekeeping).
    pub fn prune(&self) -> DbResult<usize> {
        let cutoff = self.inner.clock.now_ms() - self.inner.retention_ms;
        self.inner.db.with_tx(|tx| {
            let n = tx.execute(
                "DELETE FROM jobs WHERE state IN ('done','cancelled','failed') AND updated_at < ?1 AND id NOT IN (SELECT job_id FROM problems WHERE dismissed = 0 AND job_id IS NOT NULL)",
                [cutoff],
            )?;
            Ok(n)
        })
    }
}

/// Handed to a runner. Cheap to clone.
#[derive(Clone)]
pub struct JobContext {
    queue: JobQueue,
    pub job_id: JobId,
    pub kind: JobKind,
    pub payload: String,
    cancel: CancellationToken,
    pause: watch::Receiver<bool>,
}

impl JobContext {
    pub fn queue(&self) -> &JobQueue {
        &self.queue
    }

    pub fn db(&self) -> &Db {
        self.queue.db()
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    pub fn is_paused(&self) -> bool {
        *self.pause.borrow()
    }

    /// Wait while paused; error out when cancelled. Call between units of work.
    pub async fn checkpoint(&self) -> JobResult<()> {
        let mut pause = self.pause.clone();
        loop {
            if self.cancel.is_cancelled() {
                return Err(JobError::Cancelled);
            }
            if !*pause.borrow_and_update() {
                return Ok(());
            }
            tokio::select! {
                _ = self.cancel.cancelled() => return Err(JobError::Cancelled),
                r = pause.changed() => { if r.is_err() { return Ok(()); } }
            }
        }
    }

    pub fn set_progress(&self, done: u32, total: Option<u32>) {
        let now = self.queue.inner.clock.now_ms();
        let r = self.queue.inner.db.with_conn(|c| {
            c.execute(
                "UPDATE jobs SET done = ?2, total = COALESCE(?3, total), updated_at = ?4 WHERE id = ?1",
                params![self.job_id, done, total.map(|t| t as i64), now],
            )?;
            Ok(())
        });
        if let Err(e) = r {
            tracing::warn!(error = %e, "progress update");
        }
        self.queue.emit_jobs();
    }

    /// Persist a resume point (opaque JSON).
    pub fn set_cursor(&self, cursor: &str) -> DbResult<()> {
        self.queue.inner.db.with_conn(|c| {
            c.execute(
                "UPDATE jobs SET cursor = ?2 WHERE id = ?1",
                params![self.job_id, cursor],
            )?;
            Ok(())
        })
    }

    pub fn cursor(&self) -> DbResult<Option<String>> {
        self.queue.inner.db.with_conn(|c| {
            Ok(c.query_row(
                "SELECT cursor FROM jobs WHERE id = ?1",
                [&self.job_id],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten())
        })
    }

    /// Items not yet done/failed, in order.
    pub fn pending_items(&self) -> DbResult<Vec<JobItem>> {
        self.queue.inner.db.with_conn(|c| {
            let mut st = c.prepare_cached("SELECT seq, item, attempts FROM job_items WHERE job_id = ?1 AND state = 'pending' ORDER BY seq")?;
            let rows = st.query_map([&self.job_id], |r| {
                Ok(JobItem { seq: r.get(0)?, item: r.get(1)?, attempts: r.get::<_, i64>(2)?.max(0) as u32 })
            })?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
    }

    pub fn item_done(&self, seq: i64) -> DbResult<()> {
        let now = self.queue.inner.clock.now_ms();
        self.queue.inner.db.with_tx(|tx| {
            tx.execute(
                "UPDATE job_items SET state = 'done', attempts = attempts + 1 WHERE job_id = ?1 AND seq = ?2",
                params![self.job_id, seq],
            )?;
            tx.execute("UPDATE jobs SET done = done + 1, updated_at = ?2 WHERE id = ?1", params![self.job_id, now])?;
            Ok(())
        })?;
        self.queue.emit_jobs();
        Ok(())
    }

    pub fn item_failed(&self, seq: i64, error: &str) -> DbResult<()> {
        let now = self.queue.inner.clock.now_ms();
        self.queue.inner.db.with_tx(|tx| {
            tx.execute(
                "UPDATE job_items SET state = 'failed', attempts = attempts + 1, error = ?3 WHERE job_id = ?1 AND seq = ?2",
                params![self.job_id, seq, error],
            )?;
            tx.execute("UPDATE jobs SET failed = failed + 1, updated_at = ?2 WHERE id = ?1", params![self.job_id, now])?;
            Ok(())
        })?;
        self.queue.emit_jobs();
        Ok(())
    }

    /// Add a discovered item at run time (e.g. a playlist pin resolving its tracks).
    pub fn add_item(&self, item: &str) -> DbResult<i64> {
        self.queue.inner.db.with_tx(|tx| {
            let seq: i64 = tx.query_row("SELECT COALESCE(MAX(seq), -1) + 1 FROM job_items WHERE job_id = ?1", [&self.job_id], |r| r.get(0))?;
            tx.execute("INSERT INTO job_items(job_id, seq, item, state) VALUES (?1, ?2, ?3, 'pending')", params![self.job_id, seq, item])?;
            tx.execute(
                "UPDATE jobs SET total = (SELECT count(*) FROM job_items WHERE job_id = ?1) WHERE id = ?1",
                [&self.job_id],
            )?;
            Ok(seq)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::WallClock;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn queue() -> JobQueue {
        JobQueue::new(Db::open_in_memory().unwrap(), Arc::new(WallClock))
    }

    /// A runner that processes every item, failing those containing "bad".
    fn item_runner(delay_ms: u64) -> Arc<dyn JobRunner> {
        Arc::new(
            move |ctx: JobContext| -> BoxFuture<'static, JobResult<()>> {
                Box::pin(async move {
                    for it in ctx.pending_items()? {
                        ctx.checkpoint().await?;
                        if delay_ms > 0 {
                            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                        }
                        if it.item.contains("bad") {
                            ctx.item_failed(it.seq, "bad item")?;
                        } else {
                            ctx.item_done(it.seq)?;
                        }
                    }
                    Ok(())
                })
            },
        )
    }

    #[tokio::test]
    async fn submit_run_and_finish_done() {
        let q = queue();
        let events = Arc::new(Mutex::new(vec![]));
        let ev = events.clone();
        q.on_change(move |e| ev.lock().push(e));
        q.register(JobKind::BulkRating, 4, item_runner(0));
        let id = q
            .submit(
                JobSpec::new(JobKind::BulkRating, "Rate 3 tracks").items(vec![
                    "a".into(),
                    "b".into(),
                    "c".into(),
                ]),
            )
            .unwrap();
        let j = q.job(&id).unwrap().unwrap();
        assert_eq!(j.state, JobState::Queued);
        assert_eq!(j.total, Some(3));
        q.run_until_idle().await.unwrap();
        let j = q.job(&id).unwrap().unwrap();
        assert_eq!(j.state, JobState::Done);
        assert_eq!(j.done, 3);
        assert_eq!(j.failed, 0);
        assert!(q.problems().unwrap().is_empty());
        assert!(events
            .lock()
            .iter()
            .any(|e| matches!(e, QueueEvent::JobsChanged(_))));
    }

    #[tokio::test]
    async fn finished_with_problems_files_one_retryable_problem() {
        let q = queue();
        q.register(JobKind::BulkLove, 2, item_runner(0));
        let id = q
            .submit(JobSpec::new(JobKind::BulkLove, "Love").items(vec![
                "ok".into(),
                "bad1".into(),
                "bad2".into(),
            ]))
            .unwrap();
        q.run_until_idle().await.unwrap();
        let j = q.job(&id).unwrap().unwrap();
        assert_eq!(j.state, JobState::Failed);
        assert_eq!(j.done, 1);
        assert_eq!(j.failed, 2);
        let p = q.problems().unwrap();
        assert_eq!(p.len(), 1);
        assert!(p[0].retryable);
        assert_eq!(p[0].job_id.as_deref(), Some(id.as_str()));
        assert!(p[0].summary.contains("2 items failed"));
        assert!(p[0].detail.as_deref().unwrap().contains("bad1: bad item"));
        // retry via the problem re-queues only the failed items
        let rid = q.retry_problem(&p[0].id).unwrap().unwrap();
        assert_eq!(rid, id);
        assert!(q.problems().unwrap().is_empty());
        assert_eq!(q.job(&id).unwrap().unwrap().state, JobState::Queued);
        q.run_until_idle().await.unwrap();
        let j = q.job(&id).unwrap().unwrap();
        assert_eq!(j.state, JobState::Failed, "still bad");
        assert_eq!(j.done, 1);
        assert_eq!(j.failed, 2);
        assert_eq!(q.problems().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn whole_job_failure_and_dismiss() {
        let q = queue();
        q.register(
            JobKind::PlaylistImport,
            1,
            Arc::new(|_ctx: JobContext| -> BoxFuture<'static, JobResult<()>> {
                Box::pin(async { Err(JobError::Failed("boom".into())) })
            }),
        );
        let id = q
            .submit(JobSpec::new(JobKind::PlaylistImport, "Import"))
            .unwrap();
        q.run_until_idle().await.unwrap();
        assert_eq!(q.job(&id).unwrap().unwrap().state, JobState::Failed);
        let p = q.problems().unwrap();
        assert_eq!(p[0].detail.as_deref(), Some("boom"));
        assert!(q.dismiss_problem(&p[0].id).unwrap());
        assert!(q.problems().unwrap().is_empty());
        q.add_problem(None, "x", None, None).unwrap();
        q.add_problem(None, "y", None, None).unwrap();
        assert_eq!(q.dismiss_all_problems().unwrap(), 2);
    }

    #[tokio::test]
    async fn bounded_concurrency_per_kind() {
        let q = queue();
        let inflight = Arc::new(AtomicUsize::new(0));
        let max = Arc::new(AtomicUsize::new(0));
        let (i2, m2) = (inflight.clone(), max.clone());
        q.register(
            JobKind::Download,
            2,
            Arc::new(
                move |_ctx: JobContext| -> BoxFuture<'static, JobResult<()>> {
                    let (i, m) = (i2.clone(), m2.clone());
                    Box::pin(async move {
                        let n = i.fetch_add(1, Ordering::SeqCst) + 1;
                        m.fetch_max(n, Ordering::SeqCst);
                        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                        i.fetch_sub(1, Ordering::SeqCst);
                        Ok(())
                    })
                },
            ),
        );
        for i in 0..6 {
            q.submit(JobSpec::new(JobKind::Download, format!("dl {i}")))
                .unwrap();
        }
        q.run_until_idle().await.unwrap();
        assert_eq!(max.load(Ordering::SeqCst), 2);
        assert!(q.jobs().unwrap().iter().all(|j| j.state == JobState::Done));
    }

    #[tokio::test]
    async fn cancel_stops_at_checkpoint_and_unregistered_kinds_wait() {
        let q = queue();
        q.register(JobKind::BulkRating, 1, item_runner(10));
        let items: Vec<String> = (0..50).map(|i| i.to_string()).collect();
        let id = q
            .submit(JobSpec::new(JobKind::BulkRating, "big").items(items))
            .unwrap();
        let other = q
            .submit(JobSpec::new(JobKind::LibrarySync, "no runner yet"))
            .unwrap();
        q.schedule_once().unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(35)).await;
        assert!(q.cancel(&id).unwrap());
        q.run_until_idle().await.unwrap();
        let j = q.job(&id).unwrap().unwrap();
        assert_eq!(j.state, JobState::Cancelled);
        assert!(j.done > 0 && j.done < 50, "stopped part way: {}", j.done);
        assert_eq!(q.job(&other).unwrap().unwrap().state, JobState::Queued);
        assert!(q.has_active(JobKind::LibrarySync).unwrap());
        assert!(!q.cancel(&id).unwrap(), "already terminal");
    }

    #[tokio::test]
    async fn not_cancellable_jobs_refuse_cancel() {
        let q = queue();
        let id = q
            .submit(JobSpec::new(JobKind::OutboxFlush, "flush").not_cancellable())
            .unwrap();
        assert!(!q.cancel(&id).unwrap());
        assert_eq!(q.job(&id).unwrap().unwrap().state, JobState::Queued);
    }

    #[tokio::test]
    async fn pause_and_resume_running_job() {
        let q = queue();
        q.register(JobKind::BulkRating, 1, item_runner(5));
        let items: Vec<String> = (0..40).map(|i| i.to_string()).collect();
        let id = q
            .submit(JobSpec::new(JobKind::BulkRating, "big").items(items))
            .unwrap();
        q.schedule_once().unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        assert!(q.pause(&id).unwrap());
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        let paused = q.job(&id).unwrap().unwrap();
        assert_eq!(paused.state, JobState::Paused);
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        assert_eq!(
            q.job(&id).unwrap().unwrap().done,
            paused.done,
            "no progress while paused"
        );
        assert!(q.resume(&id).unwrap());
        q.run_until_idle().await.unwrap();
        let j = q.job(&id).unwrap().unwrap();
        assert_eq!(j.state, JobState::Done);
        assert_eq!(j.done, 40);
    }

    #[tokio::test]
    async fn pause_queued_job_then_resume_runs_it() {
        let q = queue();
        q.register(JobKind::BulkRating, 1, item_runner(0));
        let id = q
            .submit(JobSpec::new(JobKind::BulkRating, "x").items(vec!["a".into()]))
            .unwrap();
        assert!(q.pause(&id).unwrap());
        q.run_until_idle().await.unwrap();
        assert_eq!(q.job(&id).unwrap().unwrap().state, JobState::Paused);
        assert!(q.resume(&id).unwrap());
        q.run_until_idle().await.unwrap();
        assert_eq!(q.job(&id).unwrap().unwrap().state, JobState::Done);
    }

    #[tokio::test]
    async fn crash_recovery_resumes_from_cursor_and_items() {
        let db = Db::open_in_memory().unwrap();
        let q = JobQueue::new(db.clone(), Arc::new(WallClock));
        // Simulate a job left "running" by a dead process with 1 of 3 items done and a cursor.
        let id = q
            .submit(JobSpec::new(JobKind::LibrarySync, "sync").items(vec![
                "a".into(),
                "b".into(),
                "c".into(),
            ]))
            .unwrap();
        db.with_conn(|c| {
            c.execute("UPDATE jobs SET state = 'running', done = 1, cursor = '{\"offset\":500}' WHERE id = ?1", [&id])?;
            c.execute("UPDATE job_items SET state = 'done' WHERE job_id = ?1 AND seq = 0", [&id])?;
            Ok(())
        })
        .unwrap();
        assert_eq!(q.recover().unwrap(), 1);
        assert_eq!(q.job(&id).unwrap().unwrap().state, JobState::Queued);
        let seen = Arc::new(Mutex::new(vec![]));
        let s2 = seen.clone();
        q.register(
            JobKind::LibrarySync,
            1,
            Arc::new(
                move |ctx: JobContext| -> BoxFuture<'static, JobResult<()>> {
                    let seen = s2.clone();
                    Box::pin(async move {
                        seen.lock().push(ctx.cursor()?.unwrap_or_default());
                        for it in ctx.pending_items()? {
                            seen.lock().push(it.item.clone());
                            ctx.item_done(it.seq)?;
                        }
                        Ok(())
                    })
                },
            ),
        );
        q.run_until_idle().await.unwrap();
        assert_eq!(*seen.lock(), vec!["{\"offset\":500}", "b", "c"]);
        let j = q.job(&id).unwrap().unwrap();
        assert_eq!(j.state, JobState::Done);
        assert_eq!(j.done, 3);
    }

    #[tokio::test]
    async fn runtime_discovered_items_and_progress() {
        let q = queue();
        q.register(
            JobKind::Download,
            1,
            Arc::new(|ctx: JobContext| -> BoxFuture<'static, JobResult<()>> {
                Box::pin(async move {
                    ctx.add_item("x")?;
                    ctx.add_item("y")?;
                    ctx.set_progress(0, Some(2));
                    for it in ctx.pending_items()? {
                        ctx.item_done(it.seq)?;
                    }
                    Ok(())
                })
            }),
        );
        let id = q
            .submit(JobSpec::new(JobKind::Download, "pin").payload(r#"{"target":"album"}"#))
            .unwrap();
        assert_eq!(
            q.payload(&id).unwrap().as_deref(),
            Some(r#"{"target":"album"}"#)
        );
        q.run_until_idle().await.unwrap();
        let j = q.job(&id).unwrap().unwrap();
        assert_eq!((j.done, j.total, j.state), (2, Some(2), JobState::Done));
    }

    #[tokio::test]
    async fn resubmit_retry_action_and_prune() {
        let q = queue();
        q.register(JobKind::BulkRating, 1, item_runner(0));
        let pid = q
            .add_problem(
                None,
                "outbox flush failed",
                None,
                Some(RetryAction::Resubmit {
                    kind: JobKind::BulkRating,
                    label: "again".into(),
                    payload: "{}".into(),
                    items: vec!["a".into()],
                }),
            )
            .unwrap();
        let jid = q.retry_problem(&pid).unwrap().unwrap();
        q.run_until_idle().await.unwrap();
        assert_eq!(q.job(&jid).unwrap().unwrap().state, JobState::Done);
        assert_eq!(
            q.retry_problem(&pid).unwrap(),
            None,
            "dismissed problems can't be retried twice"
        );
        assert_eq!(q.prune().unwrap(), 0, "within retention");
    }

    #[test]
    fn failure_classification() {
        assert_eq!(classify_failure(true), FailureSurface::Toast);
        assert_eq!(classify_failure(false), FailureSurface::ProblemsList);
        assert_eq!(
            kind_from_name(kind_name(JobKind::FilterMaterialise)),
            Some(JobKind::FilterMaterialise)
        );
    }
}
