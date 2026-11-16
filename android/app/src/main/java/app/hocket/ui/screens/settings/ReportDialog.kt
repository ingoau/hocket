package app.hocket.ui.screens.settings

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import app.hocket.R
import app.hocket.core.api.ReportAttachmentKind
import app.hocket.diagnostics.ReportPick
import app.hocket.diagnostics.ReportStaging
import app.hocket.ui.icons.HocketIcons
import coil3.compose.AsyncImage

/**
 * "Save to Coordinator": a note, up to [ReportStaging.MAX_SCREENSHOTS] screenshots and one screen
 * recording, all picked with the system photo picker (no storage permission), in a bottom sheet.
 * Send hands them to [onSend] and closes at once; the upload then shows in the bar across the top of the app.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun ReportDialog(onSend: (note: String, picks: List<ReportPick>) -> Unit, onDismiss: () -> Unit) {
    var note by remember { mutableStateOf("") }
    val picks = remember { mutableStateListOf<ReportPick>() }
    val screenshots = picks.count { it.kind == ReportAttachmentKind.Screenshot }
    val hasRecording = picks.any { it.kind == ReportAttachmentKind.Recording }
    // The picker's limit is fixed when the launcher is made; what is over the remaining room is dropped.
    val pickScreenshots = rememberLauncherForActivityResult(ActivityResultContracts.PickMultipleVisualMedia(ReportStaging.MAX_SCREENSHOTS)) { uris ->
        val room = ReportStaging.MAX_SCREENSHOTS - picks.count { it.kind == ReportAttachmentKind.Screenshot }
        val known = picks.map { it.uri }.toSet()
        uris.filter { it !in known }.take(room.coerceAtLeast(0)).forEach { picks += ReportPick(it, ReportAttachmentKind.Screenshot) }
    }
    val pickRecording = rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri ->
        if (uri != null) {
            picks.removeAll { it.kind == ReportAttachmentKind.Recording }
            picks += ReportPick(uri, ReportAttachmentKind.Recording)
        }
    }
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true), modifier = Modifier.testTag("report.dialog")) {
        Column(
            Modifier.verticalScroll(rememberScrollState()).navigationBarsPadding().imePadding().padding(horizontal = 24.dp).padding(bottom = 16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text(stringResource(R.string.report_title), style = MaterialTheme.typography.titleLarge, modifier = Modifier.semantics { heading() })
            Text(stringResource(R.string.report_body), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            OutlinedTextField(
                value = note,
                onValueChange = { note = it.take(NOTE_MAX_CHARS) },
                label = { Text(stringResource(R.string.report_note)) },
                minLines = 3,
                maxLines = 8,
                modifier = Modifier.fillMaxWidth().testTag("report.note"),
            )
            if (picks.isNotEmpty()) {
                Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    var shot = 0
                    picks.forEachIndexed { i, pick ->
                        val label = if (pick.kind == ReportAttachmentKind.Screenshot) stringResource(R.string.report_screenshot, ++shot) else stringResource(R.string.report_recording)
                        Thumbnail(pick, label, onRemove = { picks.removeAt(i) }, tag = "report.attachment.$i")
                    }
                }
            }
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                if (screenshots < ReportStaging.MAX_SCREENSHOTS) {
                    OutlinedButton(onClick = { pickScreenshots.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly)) }, modifier = Modifier.testTag("report.addScreenshot")) {
                        Icon(HocketIcons.Filled.Add, null, Modifier.size(18.dp)); Spacer(Modifier.width(8.dp)); Text(stringResource(R.string.report_add_screenshot))
                    }
                }
                if (!hasRecording) {
                    OutlinedButton(onClick = { pickRecording.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.VideoOnly)) }, modifier = Modifier.testTag("report.addRecording")) {
                        Icon(HocketIcons.Filled.Add, null, Modifier.size(18.dp)); Spacer(Modifier.width(8.dp)); Text(stringResource(R.string.report_add_recording))
                    }
                }
            }
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End)) {
                TextButton(onClick = onDismiss, modifier = Modifier.testTag("report.cancel")) { Text(stringResource(R.string.action_cancel)) }
                Button(onClick = { onSend(note.trim(), picks.toList()) }, modifier = Modifier.testTag("report.send")) { Text(stringResource(R.string.report_send)) }
            }
        }
    }
}

/** The coordinator takes notes of at most 8000 characters. */
private const val NOTE_MAX_CHARS = 8000

@Composable
private fun Thumbnail(pick: ReportPick, label: String, onRemove: () -> Unit, tag: String) {
    Box(Modifier.size(76.dp).testTag(tag)) {
        Surface(shape = RoundedCornerShape(12.dp), color = MaterialTheme.colorScheme.surfaceContainerHighest, modifier = Modifier.padding(top = 6.dp, end = 6.dp).fillMaxSize()) {
            if (pick.kind == ReportAttachmentKind.Screenshot) {
                AsyncImage(model = pick.uri, contentDescription = label, contentScale = ContentScale.Crop, modifier = Modifier.fillMaxSize().clip(RoundedCornerShape(12.dp)))
            } else {
                Box(contentAlignment = Alignment.Center) {
                    Icon(HocketIcons.Filled.PlayArrow, contentDescription = label, tint = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.size(32.dp))
                }
            }
        }
        FilledIconButton(
            onClick = onRemove,
            shape = CircleShape,
            colors = IconButtonDefaults.filledIconButtonColors(containerColor = MaterialTheme.colorScheme.inverseSurface, contentColor = MaterialTheme.colorScheme.inverseOnSurface),
            modifier = Modifier.align(Alignment.TopEnd).size(24.dp).testTag("$tag.remove"),
        ) {
            Icon(HocketIcons.Filled.Close, contentDescription = stringResource(R.string.report_remove, label), modifier = Modifier.size(14.dp))
        }
    }
}
