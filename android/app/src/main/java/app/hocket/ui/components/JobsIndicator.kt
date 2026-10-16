package app.hocket.ui.components

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.LinearWavyProgressIndicator
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import app.hocket.R
import app.hocket.core.Commands
import app.hocket.core.api.Job
import app.hocket.core.api.JobKind
import app.hocket.core.api.JobState
import app.hocket.core.api.ServerInfo
import app.hocket.core.api.SyncProgress
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.icons.HocketIcons

/**
 * The one progress indicator near search (design: job queue). Idle: a plain activity icon; running:
 * an expressive loading indicator; finished with problems: an error badge.
 */
@Composable
fun JobsIndicator(onClick: () -> Unit, modifier: Modifier = Modifier) {
    val client = LocalCoreClient.current
    val active by client.activeJobs.collectAsStateWithLifecycleCompat()
    val problems by client.finishedWithProblems.collectAsStateWithLifecycleCompat()
    val problemList by client.problems.collectAsStateWithLifecycleCompat()
    val hasProblems = problems || problemList.isNotEmpty()
    val desc = if (hasProblems) stringResource(R.string.jobs_indicator_problems_a11y) else stringResource(R.string.jobs_indicator_a11y, active.size)
    IconButton(onClick = onClick, modifier = modifier.semantics { contentDescription = desc }) {
        when {
            active.isNotEmpty() -> LoadingIndicator(modifier = Modifier.size(28.dp))
            hasProblems -> Icon(HocketIcons.Filled.ErrorOutline, null, tint = MaterialTheme.colorScheme.error)
            else -> Icon(HocketIcons.Filled.Sync, null)
        }
    }
}

@Composable
fun JobsSheet(onDismiss: () -> Unit) {
    val client = LocalCoreClient.current
    val jobs by client.jobs.collectAsStateWithLifecycleCompat()
    val problems by client.problems.collectAsStateWithLifecycleCompat()
    val sync by client.syncProgress.collectAsStateWithLifecycleCompat()
    val server by client.server.collectAsStateWithLifecycleCompat()
    // Library syncs are summarised by the sync row (latest run only), not listed one row per run.
    val lastSyncJob = jobs.filter { it.kind == JobKind.LibrarySync }.maxByOrNull { it.createdAt }
    val others = jobs.filter { it.kind != JobKind.LibrarySync }
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)) {
        Column(Modifier.navigationBarsPadding()) {
            server?.let {
                SyncRow(it, lastSyncJob, sync)
                if (others.isNotEmpty() || problems.isNotEmpty()) Spacer(Modifier.height(8.dp))
            }
            if (server == null && others.isEmpty() && problems.isEmpty()) {
                EmptyState(stringResource(R.string.empty_jobs_title), stringResource(R.string.empty_jobs_body))
            }
            LazyColumn {
                items(others, key = { it.id }) { job -> JobRow(job) }
                if (problems.isNotEmpty()) {
                    item {
                        // The heading separates the section; no rule needed.
                        Row(Modifier.fillMaxWidth().padding(start = 24.dp, end = 24.dp, top = if (others.isEmpty()) 0.dp else 16.dp, bottom = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                            Text(stringResource(R.string.problems_title), style = MaterialTheme.typography.titleMedium, modifier = Modifier.weight(1f))
                            TextButton(onClick = { client.dispatch(app.hocket.core.api.Command.DismissAllProblems) }) { Text(stringResource(R.string.problems_dismiss_all)) }
                        }
                    }
                    items(problems, key = { it.id }) { p ->
                        Row(Modifier.fillMaxWidth().padding(horizontal = 24.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                            Icon(HocketIcons.Filled.ErrorOutline, null, tint = MaterialTheme.colorScheme.error)
                            Spacer(Modifier.size(12.dp))
                            Column(Modifier.weight(1f)) {
                                Text(p.summary, style = MaterialTheme.typography.bodyLarge)
                                p.detail?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                            }
                            if (p.retryable) IconButton(onClick = { client.dispatch(Commands.retryProblem(p.id)) }) { Icon(HocketIcons.Filled.Refresh, stringResource(R.string.action_retry)) }
                            IconButton(onClick = { client.dispatch(Commands.dismissProblem(p.id)) }) { Icon(HocketIcons.Filled.Close, stringResource(R.string.action_dismiss)) }
                        }
                    }
                }
                item { Spacer(Modifier.height(24.dp)) }
            }
        }
    }
}

@Composable
private fun JobRow(job: Job) {
    val client = LocalCoreClient.current
    Column(Modifier.fillMaxWidth().padding(horizontal = 24.dp, vertical = 8.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(job.label, style = MaterialTheme.typography.bodyLarge)
                val state = when (job.state) {
                    JobState.Queued -> stringResource(R.string.jobs_state_queued)
                    JobState.Running -> stringResource(R.string.jobs_state_running)
                    JobState.Paused -> stringResource(R.string.jobs_state_paused)
                    JobState.Done -> stringResource(R.string.jobs_state_done)
                    JobState.Failed -> stringResource(R.string.jobs_state_failed)
                    JobState.Cancelled -> stringResource(R.string.jobs_state_cancelled)
                }
                val progress = job.total?.let { stringResource(R.string.jobs_progress, job.done.toInt(), it.toInt()) } ?: job.done.toString()
                val failed = if (job.failed > 0u) stringResource(R.string.dot_separator) + stringResource(R.string.jobs_failed, job.failed.toInt()) else ""
                Text("$state${stringResource(R.string.dot_separator)}$progress$failed", style = MaterialTheme.typography.bodySmall,
                    color = if (job.state == JobState.Failed) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant)
            }
            when (job.state) {
                JobState.Running -> {
                    IconButton(onClick = { client.dispatch(Commands.pauseJob(job.id)) }) { Icon(HocketIcons.Filled.Pause, stringResource(R.string.jobs_pause)) }
                    if (job.cancellable) IconButton(onClick = { client.dispatch(Commands.cancelJob(job.id)) }) { Icon(HocketIcons.Filled.Cancel, stringResource(R.string.action_cancel)) }
                }
                JobState.Paused, JobState.Queued -> {
                    IconButton(onClick = { client.dispatch(Commands.resumeJob(job.id)) }) { Icon(HocketIcons.Filled.PlayArrow, stringResource(R.string.jobs_resume)) }
                    if (job.cancellable) IconButton(onClick = { client.dispatch(Commands.cancelJob(job.id)) }) { Icon(HocketIcons.Filled.Cancel, stringResource(R.string.action_cancel)) }
                }
                JobState.Failed, JobState.Cancelled -> IconButton(onClick = { client.dispatch(Commands.retryJob(job.id)) }) { Icon(HocketIcons.Filled.Refresh, stringResource(R.string.action_retry)) }
                JobState.Done -> Icon(HocketIcons.Filled.CheckCircle, stringResource(R.string.jobs_done), tint = MaterialTheme.colorScheme.primary)
            }
        }
        if (job.state == JobState.Running || job.state == JobState.Paused) {
            val total = job.total?.toFloat()?.takeIf { it > 0f }
            if (total != null) LinearWavyProgressIndicator(progress = { job.done.toFloat() / total }, modifier = Modifier.fillMaxWidth().padding(top = 6.dp))
            else LinearWavyProgressIndicator(modifier = Modifier.fillMaxWidth().padding(top = 6.dp))
        }
    }
}

/**
 * The server's sync as one row (design D): what it is doing or when it last finished, the library
 * size or live count beneath, one action at the end, and a thin progress line while it runs.
 */
@Composable
private fun SyncRow(server: ServerInfo, job: Job?, progress: SyncProgress?) {
    val client = LocalCoreClient.current
    val active = job?.takeIf { it.state == JobState.Running || it.state == JobState.Queued || it.state == JobState.Paused }
    // lastSync is only stamped on success, so a failed or cancelled run newer than it is the latest word.
    val unsuccessful = job?.takeIf { (it.state == JobState.Failed || it.state == JobState.Cancelled) && it.updatedAt > (server.lastSync ?: 0.0) }
    // A failed or cancelled run never reports finished, so its last progress lingers: only a job
    // that is running (or paused, for its count) owns it, and a queued one hasn't started yet.
    val live = progress?.takeIf { !it.finished && it.serverId == server.id && (active?.state == JobState.Running || active?.state == JobState.Paused) }
    val total = live?.total?.takeIf { it > 0u }
    val last = server.lastSync
    val error = MaterialTheme.colorScheme.error
    val headline = when {
        active?.state == JobState.Paused -> stringResource(R.string.sync_paused)
        active != null && live != null -> stringResource(R.string.sync_phase, live.phase)
        active != null -> stringResource(R.string.sync_waiting)
        unsuccessful?.state == JobState.Failed -> stringResource(R.string.sync_failed_when, agoText(unsuccessful.updatedAt))
        unsuccessful != null -> stringResource(R.string.sync_cancelled_when, agoText(unsuccessful.updatedAt))
        last != null -> stringResource(R.string.sync_synced_when, agoText(last))
        else -> stringResource(R.string.sync_not_synced)
    }
    val tracks = server.lastSyncTracks?.toInt()
    val supporting = when {
        // Other phases report against the track total too, so only tracks get a count.
        active != null && live != null && total != null && live.phase == "tracks" -> stringResource(R.string.sync_progress, live.done.toInt(), total.toInt())
        // After a failure the last good run's size would read as the failed run's.
        active == null && unsuccessful == null && tracks != null -> pluralStringResource(R.plurals.sync_server_tracks, tracks, server.name, tracks)
        else -> server.name
    }
    Column(Modifier.fillMaxWidth().padding(horizontal = 24.dp)) {
        Row(Modifier.heightIn(min = 64.dp), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f).padding(vertical = 8.dp).semantics(mergeDescendants = true) {}) {
                Text(headline, style = MaterialTheme.typography.titleMedium, color = if (unsuccessful?.state == JobState.Failed) error else MaterialTheme.colorScheme.onSurface)
                Text(supporting, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            when {
                active == null && unsuccessful?.state == JobState.Failed -> TextButton(onClick = { client.dispatch(Commands.retryJob(unsuccessful.id)) }) { Text(stringResource(R.string.action_retry)) }
                active == null -> IconButton(onClick = { client.dispatch(Commands.syncLibrary(server.id, full = false)) }) {
                    Icon(HocketIcons.Filled.Sync, stringResource(R.string.settings_sync_now), tint = MaterialTheme.colorScheme.primary)
                }
                active.state == JobState.Paused -> TextButton(onClick = { client.dispatch(Commands.resumeJob(active.id)) }) { Text(stringResource(R.string.jobs_resume)) }
                active.cancellable -> TextButton(onClick = { client.dispatch(Commands.cancelJob(active.id)) }) { Text(stringResource(R.string.action_cancel), color = error) }
            }
        }
        if (active != null) {
            // Other phases report against the track total too (albums by album offset), so only tracks get a fraction.
            val fraction = if (live != null && total != null && live.phase == "tracks") (live.done.toFloat() / total.toFloat()).coerceIn(0f, 1f) else null
            val line = Modifier.fillMaxWidth().padding(bottom = 8.dp).height(3.dp)
            when {
                fraction != null -> LinearProgressIndicator(progress = { fraction }, modifier = line, gapSize = 0.dp, drawStopIndicator = {})
                active.state == JobState.Paused -> LinearProgressIndicator(progress = { 0f }, modifier = line, gapSize = 0.dp, drawStopIndicator = {})
                else -> LinearProgressIndicator(modifier = line, gapSize = 0.dp)
            }
        }
    }
}

/** "just now" / "5m ago" (formatAgo alone would give "now ago"). */
@Composable
private fun agoText(epochMs: Double): String =
    if (System.currentTimeMillis() - epochMs < 60_000) stringResource(R.string.sync_just_now) else stringResource(R.string.sync_ago, formatAgo(epochMs))
