//! Save to Coordinator (`docs/reports.md`): the grant the hosted coordinator
//! sends with its diagnostics answer, and the report upload it allows, run
//! in the background and carried on across restarts from
//! `<data_dir>/report-upload/`. The HTTP side and the staging files live in
//! [`crate::reports`]; this is when to do what, and what the platform hears.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::api::*;
use crate::connect::diagnostics::ReplyPart;
use crate::core::actor::Actor;
use crate::core::{ActorMsg, Internal};
use crate::reports::{self, Failure, GrantUpdate, HeldGrant, Job, NewReport, UploadEvent};

const TARGET: &str = "hocket::reports";

/// A report that failed before anything was staged (no grant, an attachment
/// that would not do): kept as sent so Retry can try it again.
pub(crate) struct Unstaged {
    note: String,
    bundle: String,
    attachments: Vec<ReportAttachment>,
}

#[derive(Default)]
pub(crate) struct ReportsState {
    /// The coordinator's leave to make reports (never shown to the platform).
    pub grant: Option<HeldGrant>,
    /// What `Query::ReportUpload` answers.
    pub status: Option<ReportUploadStatus>,
    /// The staged upload (as of its last state change).
    job: Option<Job>,
    unstaged: Option<Unstaged>,
    /// Messages from an older task (aborted, replaced) are dropped.
    generation: u64,
    task: Option<tokio::task::AbortHandle>,
    client: Option<reqwest::Client>,
}

impl ReportsState {
    /// Stop the running task, if any; whatever it still sends is ignored.
    pub(crate) fn abort(&mut self) {
        if let Some(t) = self.task.take() {
            t.abort();
        }
        self.generation += 1;
    }

    fn uploading(&self) -> bool {
        self.status
            .as_ref()
            .is_some_and(|s| s.state == ReportUploadState::Uploading)
    }
}

/// What the upload task posts back.
pub struct UploadMsg {
    generation: u64,
    kind: UploadMsgKind,
}

enum UploadMsgKind {
    Event(UploadEvent),
    Finished {
        job: Box<Job>,
        result: Result<(), Failure>,
    },
}

impl Actor {
    fn report_dir(&self) -> PathBuf {
        reports::staging_dir(Path::new(&self.cfg.data_dir))
    }

    fn emit_report_upload(&self) {
        self.emit(Event::ReportUploadChanged {
            status: self.reports.status.clone(),
        });
    }

    /// `RoomDiagnosticsStatus::report_upload`.
    pub(crate) fn report_upload_allowed(&self) -> bool {
        reports::grant_usable(
            self.reports.grant.as_ref(),
            &self.connection_state(),
            self.now(),
        )
    }

    /// A part of an answer to this device's diagnostics request: the
    /// coordinator's first part grants (or withdraws) leave to make reports.
    pub(crate) fn on_report_grant(&mut self, part: &ReplyPart) {
        match reports::grant_update(part, &self.connection_state(), self.now()) {
            GrantUpdate::Ignore => {}
            GrantUpdate::Clear => {
                if self.reports.grant.take().is_some() {
                    tracing::info!(target: TARGET, "the coordinator no longer takes reports from this account");
                }
            }
            GrantUpdate::Hold(g) => {
                if self.reports.grant.is_none() {
                    tracing::info!(target: TARGET, "the coordinator takes reports from this account");
                }
                self.reports.grant = Some(g);
            }
        }
    }

    /// The coordinator URL setting changed: a grant from another coordinator
    /// is no good.
    pub(crate) fn coordinator_url_changed(&mut self, url: Option<&str>) {
        if self
            .reports
            .grant
            .as_ref()
            .is_some_and(|g| Some(g.coordinator_url.as_str()) != url)
        {
            self.reports.grant = None;
        }
    }

    /// `Hocket Android 0.9.0` (with the core's version when the app's differs).
    fn report_client(&self) -> String {
        let platform = match self.cfg.platform {
            Platform::Android => "Android",
            Platform::Linux => "Linux",
            Platform::MacOs => "macOS",
            Platform::Windows => "Windows",
            Platform::Coordinator => "Coordinator",
        };
        let core = env!("CARGO_PKG_VERSION");
        let app = self.cfg.app_version.trim();
        if app.is_empty() || app == core {
            format!("Hocket {platform} {core}")
        } else {
            format!("Hocket {platform} {app} (core {core})")
        }
    }

    fn report_http(&mut self) -> Result<reqwest::Client, String> {
        if let Some(c) = &self.reports.client {
            return Ok(c.clone());
        }
        let c = reports::http_client(&format!("{} reports", self.report_client()))
            .map_err(|e| e.to_string())?;
        self.reports.client = Some(c.clone());
        Ok(c)
    }

    /// `Command::StartReportUpload`.
    pub(crate) fn start_report_upload(
        &mut self,
        note: String,
        bundle: String,
        attachments: Vec<ReportAttachment>,
    ) {
        if self.reports.uploading() {
            tracing::warn!(target: TARGET, "a report is already uploading; ignoring another");
            return;
        }
        // A new report replaces what is left of the last one.
        self.discard_report_upload();
        let now = self.now();
        let grant = self.reports.grant.clone().filter(|g| g.valid(now));
        let base = grant
            .as_ref()
            .and_then(|g| reports::base_url(&g.coordinator_url));
        let (grant, base) = match (grant, base) {
            (Some(g), Some(b)) => (g, b),
            (g, _) => {
                let f = if g.is_none() {
                    Failure::new(
                        ReportUploadError::NotAllowed,
                        "no leave from the coordinator: open diagnostics again while connected to it",
                    )
                } else {
                    Failure::new(
                        ReportUploadError::Server,
                        "the coordinator URL has no HTTP form",
                    )
                };
                return self.fail_unstaged(
                    Unstaged {
                        note,
                        bundle,
                        attachments,
                    },
                    f,
                );
            }
        };
        let dir = self.report_dir();
        let staged = reports::stage(
            &dir,
            NewReport {
                note: &note,
                bundle: &bundle,
                attachments: &attachments,
                base_url: base,
                client: self.report_client(),
                device: self.cfg.device_name.clone(),
                started_at: now,
            },
        );
        match staged {
            Ok(job) => {
                tracing::info!(
                    target: TARGET,
                    files = job.files.len(),
                    bytes = job.total_bytes(),
                    "saving a report on the coordinator"
                );
                self.run_report_upload(job, Some(grant.token));
            }
            Err(f) => {
                reports::clean_up(&dir, None);
                self.fail_unstaged(
                    Unstaged {
                        note,
                        bundle,
                        attachments,
                    },
                    f,
                );
            }
        }
    }

    fn fail_unstaged(&mut self, unstaged: Unstaged, f: Failure) {
        tracing::warn!(target: TARGET, error = ?f.error, detail = %f.detail, "report not started");
        self.reports.unstaged = Some(unstaged);
        self.reports.status = Some(ReportUploadStatus {
            state: ReportUploadState::Failed,
            sent_bytes: 0.0,
            total_bytes: 0.0,
            report_id: None,
            error: Some(f.error),
            error_detail: Some(f.detail),
            started_at: self.now(),
        });
        self.emit_report_upload();
    }

    /// Mark the staged job failed (in `job.json` too) and say so.
    fn fail_job(&mut self, mut job: Job, f: Failure) {
        tracing::warn!(
            target: TARGET,
            error = ?f.error,
            detail = %f.detail,
            report_id = job.report_id.as_deref().unwrap_or("-"),
            "report upload failed"
        );
        job.state = ReportUploadState::Failed;
        job.error = Some(f.error);
        job.error_detail = Some(f.detail);
        if let Err(e) = reports::save_job(&self.report_dir(), &job) {
            tracing::warn!(target: TARGET, error = %e, "saving the report upload's job.json");
        }
        self.reports.status = Some(job.status());
        self.reports.job = Some(job);
        self.emit_report_upload();
    }

    /// Upload `job` in the background from wherever the coordinator has it.
    fn run_report_upload(&mut self, mut job: Job, grant: Option<String>) {
        self.reports.abort();
        let generation = self.reports.generation;
        let client = match self.report_http() {
            Ok(c) => c,
            Err(e) => {
                return self.fail_job(
                    job,
                    Failure::new(ReportUploadError::Network, format!("HTTP client: {e}")),
                )
            }
        };
        let dir = self.report_dir();
        job.state = ReportUploadState::Uploading;
        job.error = None;
        job.error_detail = None;
        if let Err(e) = reports::save_job(&dir, &job) {
            tracing::warn!(target: TARGET, error = %e, "saving the report upload's job.json");
        }
        self.reports.status = Some(job.status());
        self.reports.job = Some(job.clone());
        self.emit_report_upload();
        let tx = self.tx.clone();
        let post = move |kind| {
            let _ = tx.send(ActorMsg::Internal(Internal::ReportUpload(UploadMsg {
                generation,
                kind,
            })));
        };
        let post_event = post.clone();
        let on: reports::OnEvent = Arc::new(move |ev| post_event(UploadMsgKind::Event(ev)));
        let handle = self.rt.spawn(async move {
            let result = reports::upload(
                &client,
                &dir,
                &mut job,
                grant.as_deref(),
                &reports::BACKOFF,
                on,
            )
            .await;
            post(UploadMsgKind::Finished {
                job: Box::new(job),
                result,
            });
        });
        self.reports.task = Some(handle.abort_handle());
    }

    pub(crate) fn on_report_upload(&mut self, msg: UploadMsg) {
        if msg.generation != self.reports.generation {
            return;
        }
        match msg.kind {
            UploadMsgKind::Event(UploadEvent::Created { report_id }) => {
                if let Some(j) = &mut self.reports.job {
                    j.report_id = Some(report_id.clone());
                }
                if let Some(s) = &mut self.reports.status {
                    s.report_id = Some(report_id);
                }
                self.emit_report_upload();
            }
            UploadMsgKind::Event(UploadEvent::Sent { sent }) => {
                if let Some(s) = &mut self.reports.status {
                    if s.state == ReportUploadState::Uploading && s.sent_bytes != sent as f64 {
                        s.sent_bytes = sent as f64;
                        self.emit_report_upload();
                    }
                }
            }
            UploadMsgKind::Finished { job, result } => {
                self.reports.task = None;
                let mut job = *job;
                match result {
                    Ok(()) => {
                        tracing::info!(
                            target: TARGET,
                            report_id = job.report_id.as_deref().unwrap_or("-"),
                            "report saved on the coordinator"
                        );
                        reports::clean_up(&self.report_dir(), Some(&job));
                        job.state = ReportUploadState::Done;
                        for f in &mut job.files {
                            f.received = f.size;
                        }
                        self.reports.status = Some(job.status());
                        self.reports.job = None;
                        self.emit_report_upload();
                    }
                    Err(f) => self.fail_job(job, f),
                }
            }
        }
    }

    /// `Command::RetryReportUpload`.
    pub(crate) fn retry_report_upload(&mut self) {
        let failed = self
            .reports
            .status
            .as_ref()
            .is_some_and(|s| s.state == ReportUploadState::Failed);
        if !failed {
            tracing::info!(target: TARGET, "no failed report upload to retry");
            return;
        }
        if let Some(u) = self.reports.unstaged.take() {
            self.reports.status = None;
            return self.start_report_upload(u.note, u.bundle, u.attachments);
        }
        let Some(job) = self.reports.job.clone() else {
            return;
        };
        let now = self.now();
        // A grant from the coordinator the report is going to, if there is one.
        let grant = self
            .reports
            .grant
            .as_ref()
            .filter(|g| {
                g.valid(now)
                    && reports::base_url(&g.coordinator_url).as_deref()
                        == Some(job.base_url.as_str())
            })
            .map(|g| g.token.clone());
        if job.report_id.is_none() && grant.is_none() {
            return self.fail_job(
                job,
                Failure::new(
                    ReportUploadError::NotAllowed,
                    "no leave from the coordinator to make the report: open diagnostics again while connected to it",
                ),
            );
        }
        tracing::info!(target: TARGET, "retrying the report upload");
        self.run_report_upload(job, grant);
    }

    /// Stop and forget the upload, deleting what was staged and the
    /// platform's copies. No event.
    fn discard_report_upload(&mut self) {
        self.reports.abort();
        if let Some(u) = self.reports.unstaged.take() {
            for a in u.attachments.iter().filter(|a| a.delete_after) {
                reports::remove_file(&a.path);
            }
        }
        let job = self.reports.job.take();
        let dir = self.report_dir();
        // job.json has the upload key from the moment the report exists.
        let saved = reports::load_job(&dir).ok().flatten();
        reports::clean_up(&dir, job.as_ref());
        self.reports.status = None;
        if let (Some(job), Ok(client)) = (saved.or(job), self.report_http()) {
            if job.report_id.is_some() && job.state != ReportUploadState::Done {
                self.rt
                    .spawn(async move { reports::abandon(&client, &job).await });
            }
        }
    }

    /// `Command::DismissReportUpload`.
    pub(crate) fn dismiss_report_upload(&mut self) {
        if self.reports.status.is_some() {
            tracing::info!(target: TARGET, "report upload dismissed");
        }
        self.discard_report_upload();
        self.emit_report_upload();
    }

    /// At start: carry on with an upload `job.json` says was running, or
    /// show the one that failed (so the platform offers Retry).
    pub(crate) fn restore_report_upload(&mut self) {
        let dir = self.report_dir();
        let job = match reports::load_job(&dir) {
            Ok(Some(job)) => job,
            Ok(None) => return,
            Err(e) => {
                tracing::warn!(target: TARGET, error = %e, "unreadable report upload job; dropping it");
                reports::clean_up(&dir, None);
                return;
            }
        };
        match job.state {
            ReportUploadState::Uploading if job.report_id.is_none() => self.fail_job(
                job,
                Failure::new(
                    ReportUploadError::NotAllowed,
                    "the app stopped before the coordinator made the report: open diagnostics again while connected to it, then retry",
                ),
            ),
            ReportUploadState::Uploading => {
                tracing::info!(target: TARGET, "resuming the report upload");
                self.run_report_upload(job, None);
            }
            ReportUploadState::Failed => {
                self.reports.status = Some(job.status());
                self.reports.job = Some(job);
                self.emit_report_upload();
            }
            ReportUploadState::Done => reports::clean_up(&dir, Some(&job)),
        }
    }
}
