package app.hocket.ui.components

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.R
import app.hocket.core.api.ReportUploadError
import app.hocket.core.api.ReportUploadState
import app.hocket.core.api.ReportUploadStatus
import app.hocket.core.client.CoreClient
import app.hocket.diagnostics.ReportUploadService
import app.hocket.ui.icons.HocketIcons
import kotlinx.coroutines.delay

/** How long the Done bar stays before it dismisses itself. */
const val REPORT_DONE_SHOWN_MS = 8_000L

/**
 * The bar across the top of the app while a "Save to Coordinator" report uploads, and once it has
 * finished (`docs/reports.md`): Cancel while uploading; the report id with Copy when done (gone by
 * itself after [REPORT_DONE_SHOWN_MS]); Retry and Dismiss when it failed.
 */
@Composable
fun ReportUploadBarHost(client: CoreClient, insets: WindowInsets?) {
    val status by client.reportUpload.collectAsStateWithLifecycle()
    val s = status ?: return
    val context = LocalContext.current
    val snackbar = LocalAppSnackbar.current
    val copied = stringResource(R.string.report_upload_id_copied)
    LaunchedEffect(s.state, s.reportId) {
        if (s.state == ReportUploadState.Done) {
            delay(REPORT_DONE_SHOWN_MS)
            client.dismissReportUpload()
        }
    }
    ReportUploadBar(
        s,
        insets = insets,
        onCancel = client::dismissReportUpload,
        onRetry = client::retryReportUpload,
        onDismiss = client::dismissReportUpload,
        onCopy = { id ->
            val ok = runCatching { (context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager).setPrimaryClip(ClipData.newPlainText("Hocket report", id)) }.isSuccess
            if (ok) snackbar?.let { sb -> sb.launch { sb.host.showSnackbar(copied) } }
        },
    )
}

@Composable
fun ReportUploadBar(
    status: ReportUploadStatus,
    insets: WindowInsets?,
    onCancel: () -> Unit,
    onRetry: () -> Unit,
    onDismiss: () -> Unit,
    onCopy: (String) -> Unit,
) {
    val failed = status.state == ReportUploadState.Failed
    val (bg, fg) = if (failed) MaterialTheme.colorScheme.errorContainer to MaterialTheme.colorScheme.onErrorContainer
    else MaterialTheme.colorScheme.secondaryContainer to MaterialTheme.colorScheme.onSecondaryContainer
    val percent = ReportUploadService.percent(status)
    val message = when (status.state) {
        ReportUploadState.Uploading -> if (percent != null) stringResource(R.string.report_upload_progress, percent) else stringResource(R.string.report_upload_starting)
        ReportUploadState.Done -> stringResource(R.string.report_upload_done, status.reportId.orEmpty())
        ReportUploadState.Failed -> stringResource(
            when (status.error) {
                ReportUploadError.NotAllowed -> R.string.report_upload_failed_not_allowed
                ReportUploadError.Unreadable -> R.string.report_upload_failed_unreadable
                ReportUploadError.TooLarge -> R.string.report_upload_failed_too_large
                ReportUploadError.Network -> R.string.report_upload_failed_network
                ReportUploadError.Server -> R.string.report_upload_failed_server
                null -> R.string.report_upload_failed
            },
        )
    }
    // Announced when the state changes, not on every percent (the progress bar reads that out).
    val announced = if (status.state == ReportUploadState.Uploading) stringResource(R.string.report_upload_starting) else message
    Column(
        Modifier.fillMaxWidth().background(bg).then(if (insets != null) Modifier.windowInsetsPadding(insets) else Modifier)
            .padding(start = 16.dp, end = 4.dp, top = 4.dp, bottom = 4.dp).testTag("report.bar"),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(
                when (status.state) {
                    ReportUploadState.Uploading -> HocketIcons.Filled.Dns
                    ReportUploadState.Done -> HocketIcons.Filled.CheckCircle
                    ReportUploadState.Failed -> HocketIcons.Filled.ErrorOutline
                },
                null, tint = fg, modifier = Modifier.size(20.dp),
            )
            Spacer(Modifier.width(12.dp))
            Text(
                message,
                style = MaterialTheme.typography.bodyMedium,
                color = fg,
                modifier = Modifier.weight(1f).padding(vertical = 8.dp)
                    .semantics { liveRegion = LiveRegionMode.Polite; contentDescription = announced }.testTag("report.bar.message"),
            )
            Row(horizontalArrangement = Arrangement.spacedBy(0.dp)) {
                when (status.state) {
                    ReportUploadState.Uploading -> TextButton(onClick = onCancel, modifier = Modifier.testTag("report.bar.cancel")) { Text(stringResource(R.string.report_upload_cancel), color = fg) }
                    ReportUploadState.Done -> {
                        val id = status.reportId
                        if (id != null) TextButton(onClick = { onCopy(id) }, modifier = Modifier.testTag("report.bar.copy")) { Text(stringResource(R.string.report_upload_copy_id), color = fg) }
                    }
                    ReportUploadState.Failed -> {
                        TextButton(onClick = onRetry, modifier = Modifier.testTag("report.bar.retry")) { Text(stringResource(R.string.report_upload_retry), color = fg) }
                        TextButton(onClick = onDismiss, modifier = Modifier.testTag("report.bar.dismiss")) { Text(stringResource(R.string.report_upload_dismiss), color = fg) }
                    }
                }
            }
        }
        if (status.state == ReportUploadState.Uploading) {
            val mod = Modifier.fillMaxWidth().padding(end = 12.dp, bottom = 6.dp).testTag("report.bar.progress")
            if (percent != null) LinearProgressIndicator(progress = { (status.sentBytes / status.totalBytes).toFloat().coerceIn(0f, 1f) }, modifier = mod, color = fg, trackColor = fg.copy(alpha = 0.24f))
            else LinearProgressIndicator(modifier = mod, color = fg, trackColor = fg.copy(alpha = 0.24f))
        }
    }
}
