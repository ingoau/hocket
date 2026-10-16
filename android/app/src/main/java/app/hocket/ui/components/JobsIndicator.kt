package app.hocket.ui.components

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.ContainedLoadingIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearWavyProgressIndicator
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.graphics.Color
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
    // Library syncs are summarised by the sync card (last result only), not listed one row per run.
    val lastSyncJob = jobs.filter { it.kind == JobKind.LibrarySync }.maxByOrNull { it.createdAt }
    val others = jobs.filter { it.kind != JobKind.LibrarySync }
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)) {
        Column(Modifier.navigationBarsPadding()) {
            Text(stringResource(R.string.jobs_title), style = MaterialTheme.typography.titleLarge, modifier = Modifier.padding(horizontal = 24.dp, vertical = 8.dp))
            server?.let { SyncCard(it, lastSyncJob, sync) }
            if (server == null && others.isEmpty() && problems.isEmpty()) {
                EmptyState(stringResource(R.string.empty_jobs_title), stringResource(R.string.empty_jobs_body))
            }
            LazyColumn {
                items(others, key = { it.id }) { job -> JobRow(job) }
                if (problems.isNotEmpty()) {
                    item {
                        HorizontalDivider()
                        Row(Modifier.fillMaxWidth().padding(horizontal = 24.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
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
 * The server's sync at a glance: the last result, how long ago, and how many tracks in how long
 * (or the live phase while one runs), with a Sync / Cancel button. Replaces a row per past run.
 */
@Composable
private fun SyncCard(server: ServerInfo, job: Job?, progress: SyncProgress?) {
    val client = LocalCoreClient.current
    val active = job?.takeIf { it.state == JobState.Running || it.state == JobState.Queued || it.state == JobState.Paused }
    val running = active != null
    Surface(
        color = MaterialTheme.colorScheme.surfaceContainerHigh,
        shape = MaterialTheme.shapes.large,
        modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
    ) {
        Column(Modifier.padding(16.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(HocketIcons.Filled.Sync, null)
                Spacer(Modifier.size(12.dp))
                Text(server.name, style = MaterialTheme.typography.titleMedium, modifier = Modifier.weight(1f))
                if (active != null && active.cancellable) TextButton(onClick = { client.dispatch(Commands.cancelJob(active.id)) }) {
                    Text(stringResource(R.string.action_cancel), color = MaterialTheme.colorScheme.error)
                } else if (!running) OutlinedButton(onClick = { client.dispatch(Commands.syncLibrary(server.id, full = false)) }) {
                    Text(stringResource(R.string.sync_action))
                }
            }
            Spacer(Modifier.height(12.dp))
            if (running) {
                val p = progress?.takeIf { !it.finished && it.serverId == server.id }
                val total = p?.total
                val phase = p?.let { stringResource(R.string.sync_phase, it.phase) } ?: stringResource(R.string.jobs_state_queued)
                val count = if (p != null && total != null) stringResource(R.string.dot_separator) + stringResource(R.string.sync_progress, p.done.toInt(), total.toInt()) else ""
                Text("$phase$count", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                if (p != null && total != null && total > 0u) LinearWavyProgressIndicator(progress = { p.done.toFloat() / total.toFloat() }, modifier = Modifier.fillMaxWidth().padding(top = 8.dp))
                else LinearWavyProgressIndicator(modifier = Modifier.fillMaxWidth().padding(top = 8.dp))
            } else {
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    when (job?.state) {
                        JobState.Done -> SyncChip(stringResource(R.string.sync_successful))
                        JobState.Failed -> SyncChip(stringResource(R.string.sync_failed), MaterialTheme.colorScheme.error)
                        JobState.Cancelled -> SyncChip(stringResource(R.string.jobs_state_cancelled))
                        else -> {}
                    }
                    val last = server.lastSync
                    if (last == null) SyncChip(stringResource(R.string.settings_never_synced))
                    else if (System.currentTimeMillis() - last < 60_000) SyncChip(stringResource(R.string.sync_just_now))
                    else SyncChip(stringResource(R.string.sync_ago, formatAgo(last)))
                    // The job is submitted when the sync starts and lastSync stamped when it finishes.
                    job?.takeIf { it.state == JobState.Done }?.let { done ->
                        val tracks = (done.total ?: done.done).toInt()
                        val took = last?.let { it - done.createdAt }?.takeIf { it >= 0.0 }
                        SyncChip(
                            if (took != null) stringResource(R.string.sync_tracks_in, tracks, formatElapsed(took.toLong()))
                            else stringResource(R.string.sync_tracks, tracks),
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun SyncChip(text: String, color: Color = MaterialTheme.colorScheme.onSurface) {
    Surface(color = MaterialTheme.colorScheme.surfaceContainerHighest, shape = MaterialTheme.shapes.small) {
        Text(text, style = MaterialTheme.typography.labelLarge, color = color, modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp))
    }
}

/** "40 s", "3 min 5 s" for how long a sync took. */
@Composable
private fun formatElapsed(ms: Long): String {
    val s = (ms / 1000).coerceAtLeast(1)
    return if (s < 60) stringResource(R.string.sync_seconds, s.toInt()) else stringResource(R.string.sync_minutes_seconds, (s / 60).toInt(), (s % 60).toInt())
}
