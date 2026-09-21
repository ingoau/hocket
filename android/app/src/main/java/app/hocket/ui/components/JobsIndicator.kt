package app.hocket.ui.components

import androidx.compose.foundation.layout.Box
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
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Cancel
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.ErrorOutline
import androidx.compose.material.icons.filled.Pause
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Sync
import androidx.compose.material3.ContainedLoadingIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
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
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import app.hocket.R
import app.hocket.core.Commands
import app.hocket.core.api.Job
import app.hocket.core.api.JobState
import app.hocket.ui.LocalCoreClient

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
            hasProblems -> Icon(Icons.Filled.ErrorOutline, null, tint = MaterialTheme.colorScheme.error)
            else -> Icon(Icons.Filled.Sync, null)
        }
    }
}

@Composable
fun JobsSheet(onDismiss: () -> Unit) {
    val client = LocalCoreClient.current
    val jobs by client.jobs.collectAsStateWithLifecycleCompat()
    val problems by client.problems.collectAsStateWithLifecycleCompat()
    val sync by client.syncProgress.collectAsStateWithLifecycleCompat()
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)) {
        Column(Modifier.navigationBarsPadding()) {
            Text(stringResource(R.string.jobs_title), style = MaterialTheme.typography.titleLarge, modifier = Modifier.padding(horizontal = 24.dp, vertical = 8.dp))
            sync?.takeIf { !it.finished }?.let { s ->
                Row(Modifier.padding(horizontal = 24.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                    ContainedLoadingIndicator(modifier = Modifier.size(40.dp))
                    Spacer(Modifier.size(12.dp))
                    Column {
                        Text(stringResource(R.string.sync_phase, s.phase), style = MaterialTheme.typography.bodyLarge)
                        s.total?.let { Text(stringResource(R.string.sync_progress, s.done.toInt(), it.toInt()), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                    }
                }
            }
            if (jobs.isEmpty() && problems.isEmpty()) {
                EmptyState(stringResource(R.string.empty_jobs_title), stringResource(R.string.empty_jobs_body))
            }
            LazyColumn {
                items(jobs, key = { it.id }) { job -> JobRow(job) }
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
                            Icon(Icons.Filled.ErrorOutline, null, tint = MaterialTheme.colorScheme.error)
                            Spacer(Modifier.size(12.dp))
                            Column(Modifier.weight(1f)) {
                                Text(p.summary, style = MaterialTheme.typography.bodyLarge)
                                p.detail?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                            }
                            if (p.retryable) IconButton(onClick = { client.dispatch(Commands.retryProblem(p.id)) }) { Icon(Icons.Filled.Refresh, stringResource(R.string.action_retry)) }
                            IconButton(onClick = { client.dispatch(Commands.dismissProblem(p.id)) }) { Icon(Icons.Filled.Close, stringResource(R.string.action_dismiss)) }
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
                    IconButton(onClick = { client.dispatch(Commands.pauseJob(job.id)) }) { Icon(Icons.Filled.Pause, stringResource(R.string.jobs_pause)) }
                    if (job.cancellable) IconButton(onClick = { client.dispatch(Commands.cancelJob(job.id)) }) { Icon(Icons.Filled.Cancel, stringResource(R.string.action_cancel)) }
                }
                JobState.Paused, JobState.Queued -> {
                    IconButton(onClick = { client.dispatch(Commands.resumeJob(job.id)) }) { Icon(Icons.Filled.PlayArrow, stringResource(R.string.jobs_resume)) }
                    if (job.cancellable) IconButton(onClick = { client.dispatch(Commands.cancelJob(job.id)) }) { Icon(Icons.Filled.Cancel, stringResource(R.string.action_cancel)) }
                }
                JobState.Failed, JobState.Cancelled -> IconButton(onClick = { client.dispatch(Commands.retryJob(job.id)) }) { Icon(Icons.Filled.Refresh, stringResource(R.string.action_retry)) }
                JobState.Done -> Icon(Icons.Filled.CheckCircle, stringResource(R.string.jobs_done), tint = MaterialTheme.colorScheme.primary)
            }
        }
        if (job.state == JobState.Running || job.state == JobState.Paused) {
            val total = job.total?.toFloat()?.takeIf { it > 0f }
            if (total != null) LinearWavyProgressIndicator(progress = { job.done.toFloat() / total }, modifier = Modifier.fillMaxWidth().padding(top = 6.dp))
            else LinearWavyProgressIndicator(modifier = Modifier.fillMaxWidth().padding(top = 6.dp))
        }
    }
}
