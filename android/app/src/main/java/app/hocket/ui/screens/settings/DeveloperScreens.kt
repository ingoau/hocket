package app.hocket.ui.screens.settings

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.navigation.NavHostController
import app.hocket.R
import app.hocket.core.SettingKeys
import app.hocket.core.api.DeviceInfo
import app.hocket.core.api.DiagnosticsSourceState
import app.hocket.core.api.DiagnosticsSourceStatus
import app.hocket.core.api.Platform
import app.hocket.core.api.RoomDiagnosticsStatus
import app.hocket.core.api.RoomDiagnosticsUnavailable
import app.hocket.core.client.CoreClient
import app.hocket.diagnostics.DiagnosticsBundle
import app.hocket.diagnostics.DiagnosticsReport
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.components.LocalAppSnackbar
import app.hocket.ui.components.formatBytes
import app.hocket.ui.icons.HocketIcons
import app.hocket.ui.nav.Route
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.util.Locale

/** Advanced: developer mode (detailed logs, shake to open diagnostics) and the developer diagnostics page. */
@Composable
fun AdvancedSettingsScreen(nav: NavHostController) {
    val devMode = setting(SettingKeys.DEVELOPER_MODE)
    SubScreen(nav, stringResource(R.string.settings_category_advanced)) {
        SwitchRow(stringResource(R.string.settings_developer_mode), devMode.bool ?: false, { devMode.setBool(it) }, stringResource(R.string.settings_developer_mode_body), devMode.scope, tag = "developer.mode")
        SettingRow(stringResource(R.string.settings_developer_diagnostics), stringResource(R.string.settings_developer_diagnostics_body), onClick = { nav.navigate(Route.DeveloperDiagnostics) }, tag = "developer.open") { Chevron() }
    }
}

/**
 * The clipboard goes through a binder transaction (a 1 MB buffer shared by the whole process, and
 * UTF-16 doubles the size); larger bundles are copied from their end.
 */
private const val CLIPBOARD_MAX_CHARS = 100_000

/**
 * Developer diagnostics: the device and app, the core's diagnostics and the newest log lines, with
 * Share (the share sheet, as a text file), Save (a document the user picks) and Copy. With developer
 * mode on, the page asks the other devices in the Connect room and its coordinator for theirs as it
 * opens (and on Refresh), shows where each answer stands, and holds the actions until they are in:
 * every action then carries them. Reached from Advanced, or by shaking the phone in developer mode.
 * [collect] and [collectOthers] are swappable for tests (no logcat or native core under Robolectric).
 */
@Composable
fun DeveloperDiagnosticsScreen(
    nav: NavHostController,
    collect: suspend (Context, CoreClient) -> DiagnosticsReport = { context, client -> DiagnosticsBundle.collect(context, client) },
    collectOthers: suspend (CoreClient) -> String = { DiagnosticsBundle.otherDevices(it) },
) {
    val client = LocalCoreClient.current
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val snackbar = LocalAppSnackbar.current
    val devMode = setting(SettingKeys.DEVELOPER_MODE)
    val on = devMode.bool ?: false
    var report by remember { mutableStateOf<DiagnosticsReport?>(null) }
    var refreshes by remember { mutableIntStateOf(0) }
    var busy by remember { mutableStateOf(false) }
    /** The other devices' bundles, once collected; every action carries them. */
    var others by remember { mutableStateOf<String?>(null) }
    var collecting by remember { mutableStateOf(false) }
    var collections by remember { mutableIntStateOf(0) }
    /** The collection shown before this page asked again: never shown as this one's. */
    var staleId by remember { mutableStateOf<String?>(null) }
    val roomStatus by client.roomDiagnostics.collectAsState()
    LaunchedEffect(refreshes) {
        report = try {
            collect(context, client)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            null
        }
    }
    // As the page opens, on Refresh, and whenever developer mode comes on (other devices only ever
    // answer while it is on here too).
    LaunchedEffect(on, collections) {
        others = null
        if (!on) {
            collecting = false
            return@LaunchedEffect
        }
        staleId = client.roomDiagnostics.value?.requestId
        collecting = true
        others = try {
            collectOthers(client)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            "Other devices: not collected (${e.message ?: e.javaClass.simpleName})\n"
        }
        collecting = false
    }
    val waiting = on && collecting
    val enabled = !busy && !waiting

    val notify: (String) -> Unit = { message -> snackbar?.let { s -> s.launch { s.host.showSnackbar(message) } } }
    val savedText = stringResource(R.string.developer_saved)
    val failedText = stringResource(R.string.developer_save_failed)
    val copiedText = stringResource(R.string.developer_copied)
    val copiedTruncatedText = stringResource(R.string.developer_copied_truncated)
    val copyFailedText = stringResource(R.string.developer_copy_failed)
    val shareTitle = stringResource(R.string.developer_share_title)
    /** Collects a fresh report (with the other devices' bundles), runs [action] on it, then refreshes the preview to match. */
    fun withFresh(action: suspend (DiagnosticsReport) -> Unit) {
        if (!enabled) return
        busy = true
        val o = if (on) others else null
        scope.launch {
            try {
                // Let the previous report go first: two of them (and the bundle text) at once is a lot
                // of memory with a big debug log.
                report = null
                val here = collect(context, client)
                report = here
                // copy() counts the lines again: off the main thread, the logs can be tens of megabytes.
                val fresh = if (o != null) withContext(Dispatchers.Default) { here.copy(otherDevices = o) } else here
                action(fresh)
            } catch (e: CancellationException) {
                throw e
            } catch (e: OutOfMemoryError) {
                notify(failedText)
            } catch (e: Exception) {
                notify(failedText + ": " + (e.message ?: e.javaClass.simpleName))
            } finally {
                busy = false
            }
        }
    }
    var pendingSave by remember { mutableStateOf<DiagnosticsReport?>(null) }
    val saveLauncher = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("text/plain")) { uri ->
        val toSave = pendingSave
        pendingSave = null
        if (uri == null || toSave == null) return@rememberLauncherForActivityResult
        scope.launch { notify(if (DiagnosticsBundle.writeTo(context, uri, toSave)) savedText else failedText) }
    }

    SubScreen(nav, stringResource(R.string.developer_title)) {
        Text(
            stringResource(R.string.developer_subtitle),
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp),
        )
        Surface(
            shape = RoundedCornerShape(24.dp),
            color = if (on) MaterialTheme.colorScheme.secondaryContainer else MaterialTheme.colorScheme.surfaceContainer,
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp).testTag("developer.modeCard"),
        ) {
            Row(Modifier.padding(horizontal = 16.dp, vertical = 14.dp), verticalAlignment = Alignment.CenterVertically) {
                Icon(HocketIcons.Filled.Code, null, tint = MaterialTheme.colorScheme.primary)
                Spacer(Modifier.width(14.dp))
                Text(stringResource(if (on) R.string.developer_mode_on else R.string.developer_mode_off), style = MaterialTheme.typography.bodyMedium, modifier = Modifier.weight(1f))
                Spacer(Modifier.width(12.dp))
                Switch(checked = on, onCheckedChange = { devMode.setBool(it) }, modifier = Modifier.testTag("developer.modeSwitch"))
            }
        }
        FlowRow(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Button(onClick = { withFresh { DiagnosticsBundle.share(context, it, shareTitle) } }, enabled = enabled, modifier = Modifier.testTag("developer.share")) {
                Icon(HocketIcons.Filled.Share, null, Modifier.size(18.dp)); Spacer(Modifier.width(8.dp)); Text(stringResource(R.string.developer_share))
            }
            FilledTonalButton(onClick = { withFresh { pendingSave = it; saveLauncher.launch(it.fileName) } }, enabled = enabled, modifier = Modifier.testTag("developer.save")) {
                Icon(HocketIcons.Filled.Save, null, Modifier.size(18.dp)); Spacer(Modifier.width(8.dp)); Text(stringResource(R.string.developer_save))
            }
            FilledTonalButton(onClick = {
                withFresh {
                    val (clip, truncated) = withContext(Dispatchers.Default) {
                        val text = it.text
                        if (text.length <= CLIPBOARD_MAX_CHARS) return@withContext text to false
                        // This phone's newest lines, then the start of the other devices' part (the
                        // room and the first devices), each in its own share of the room.
                        val others = it.otherDevices.trimEnd()
                        val othersBudget = if (others.isEmpty()) 0 else minOf(others.length, CLIPBOARD_MAX_CHARS / 2)
                        val ownBudget = CLIPBOARD_MAX_CHARS - othersBudget - 8
                        val own = it.ownText.let { t -> if (t.length <= ownBudget) t else it.head + "\n…\n" + t.takeLast(ownBudget - it.head.length - 3) }
                        val tail = if (others.isEmpty()) "" else "\n" + others.take(othersBudget) + if (othersBudget < others.length) "\n…\n" else "\n"
                        (own + tail) to true
                    }
                    val copied = runCatching { (context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager).setPrimaryClip(ClipData.newPlainText(it.fileName, clip)) }.isSuccess
                    notify(if (!copied) copyFailedText else if (truncated) copiedTruncatedText else copiedText)
                }
            }, enabled = enabled, modifier = Modifier.testTag("developer.copy")) {
                Icon(HocketIcons.Filled.ContentCopy, null, Modifier.size(18.dp)); Spacer(Modifier.width(8.dp)); Text(stringResource(R.string.developer_copy))
            }
            OutlinedButton(onClick = { refreshes++; if (on) collections++ }, enabled = enabled, modifier = Modifier.testTag("developer.refresh")) {
                Icon(HocketIcons.Filled.Refresh, null, Modifier.size(18.dp)); Spacer(Modifier.width(8.dp)); Text(stringResource(R.string.developer_refresh))
            }
        }
        if (waiting) {
            Text(
                stringResource(R.string.developer_wait_for_others),
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp).testTag("developer.collecting"),
            )
        }
        val r = report
        if (r != null) {
            Text(
                stringResource(R.string.developer_stats, r.coreLines, r.logcatLines, formatBytes(r.bytes.toDouble()), (r.windowMs / 60_000).toInt()),
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp).testTag("developer.stats"),
            )
        }
        val devices by client.devices.collectAsState()
        RoomCard(on, waiting, roomStatus?.takeIf { it.requestId != staleId }, devices)
        Block(stringResource(R.string.developer_environment), r?.head?.trimEnd() ?: stringResource(R.string.developer_loading), "developer.head")
        Block(
            stringResource(R.string.developer_log),
            r?.let { it.preview.ifEmpty { stringResource(R.string.developer_empty) } } ?: stringResource(R.string.developer_loading),
            "developer.log",
            note = r?.let { stringResource(R.string.developer_log_tail, DiagnosticsReport.PREVIEW_LINES) },
        )
    }
}

/** The other devices and the coordinator: who was asked, and where each answer stands. */
@Composable
private fun RoomCard(on: Boolean, collecting: Boolean, status: RoomDiagnosticsStatus?, devices: List<DeviceInfo>) {
    val running = collecting || (status != null && !status.done)
    val settled = status?.devices?.count { it.state != DiagnosticsSourceState.Waiting && it.state != DiagnosticsSourceState.Receiving } ?: 0
    val headline = when {
        !on || status?.unavailable == RoomDiagnosticsUnavailable.DeveloperModeOff -> stringResource(R.string.developer_others_off)
        status?.unavailable == RoomDiagnosticsUnavailable.NotConnected -> stringResource(R.string.developer_others_not_connected)
        status == null || (running && !status.roomAnswered) -> stringResource(R.string.developer_others_asking)
        !status.roomAnswered -> stringResource(R.string.developer_others_no_room)
        running -> stringResource(R.string.developer_others_collecting, settled, status.devices.size)
        status.devices.isEmpty() -> stringResource(R.string.developer_others_none)
        else -> stringResource(
            R.string.developer_others_done,
            status.devices.count { it.state == DiagnosticsSourceState.Received },
            status.devices.size,
            String.format(Locale.getDefault(), "%.1f", status.elapsedMs / 1000),
        )
    }
    Surface(shape = RoundedCornerShape(24.dp), color = MaterialTheme.colorScheme.surfaceContainer, modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp).testTag("developer.room")) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(stringResource(R.string.developer_others), style = MaterialTheme.typography.titleMedium, modifier = Modifier.semantics { heading() })
            Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.semantics(mergeDescendants = true) { liveRegion = LiveRegionMode.Polite }.testTag("developer.roomStatus")) {
                if (running && on) {
                    CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp)
                    Spacer(Modifier.width(8.dp))
                }
                Text(headline, style = MaterialTheme.typography.bodyMedium)
            }
            Text(stringResource(R.string.developer_others_hint), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            if (on && status != null && status.unavailable == null) {
                Column(verticalArrangement = Arrangement.spacedBy(4.dp), modifier = Modifier.padding(top = 4.dp)) {
                    CoordinatorRow(status, running)
                    for (d in status.devices) {
                        val platform = devices.firstOrNull { it.id == d.id }?.platform
                        val icon = when (platform) {
                            null -> HocketIcons.Filled.Devices
                            Platform.Android -> HocketIcons.Filled.PhoneAndroid
                            else -> HocketIcons.Filled.Computer
                        }
                        SourceRow(icon, d.name, d, detail = null, tag = "developer.source.${d.id}")
                    }
                }
            }
        }
    }
}

@Composable
private fun CoordinatorRow(status: RoomDiagnosticsStatus, running: Boolean) {
    val c = status.coordinator
    val name = stringResource(R.string.developer_coordinator)
    if (c != null) {
        val detail = if (c.state == DiagnosticsSourceState.Received) stringResource(if (status.coordinatorLog) R.string.developer_coordinator_log else R.string.developer_coordinator_version) else null
        SourceRow(HocketIcons.Filled.Dns, name, c, detail, "developer.source.coordinator")
        return
    }
    // It answers ahead of the room: once the room has, there is none.
    val (detail, chip, tone) = when {
        status.roomAnswered -> Triple(R.string.developer_coordinator_none, R.string.developer_state_none, ChipTone.Neutral)
        running -> Triple(R.string.developer_coordinator_waiting, R.string.developer_state_waiting, ChipTone.Neutral)
        else -> Triple(R.string.developer_others_no_room, R.string.developer_state_no_answer, ChipTone.Bad)
    }
    SourceRowLayout(HocketIcons.Filled.Dns, name, stringResource(detail), "developer.source.coordinator") {
        StateChip(stringResource(chip), tone, progress = null)
    }
}

private enum class ChipTone { Neutral, Good, Bad }

@Composable
private fun SourceRow(icon: ImageVector, name: String, s: DiagnosticsSourceStatus, detail: String?, tag: String) {
    val parts = s.parts.toInt()
    val received = s.partsReceived.toInt()
    val (label, tone) = when (s.state) {
        DiagnosticsSourceState.Waiting -> stringResource(R.string.developer_state_waiting) to ChipTone.Neutral
        DiagnosticsSourceState.Receiving -> stringResource(R.string.developer_state_receiving, received, parts) to ChipTone.Neutral
        DiagnosticsSourceState.Received -> stringResource(R.string.developer_state_received, formatBytes(s.bytes.toDouble())) to ChipTone.Good
        DiagnosticsSourceState.Declined -> stringResource(R.string.developer_state_declined) to ChipTone.Bad
        DiagnosticsSourceState.Incomplete -> stringResource(R.string.developer_state_incomplete, received, parts) to ChipTone.Bad
        DiagnosticsSourceState.NoAnswer -> stringResource(R.string.developer_state_no_answer) to ChipTone.Bad
    }
    val sub = if (s.state == DiagnosticsSourceState.Declined) s.reason else detail
    val progress = if (s.state == DiagnosticsSourceState.Receiving && parts > 0) received.toFloat() / parts else null
    SourceRowLayout(icon, name, sub, tag) { StateChip(label, tone, progress) }
}

@Composable
private fun SourceRowLayout(icon: ImageVector, name: String, sub: String?, tag: String, trailing: @Composable () -> Unit) {
    Surface(shape = RoundedCornerShape(16.dp), color = MaterialTheme.colorScheme.surfaceContainerHighest, modifier = Modifier.fillMaxWidth()) {
        Row(Modifier.semantics(mergeDescendants = true) {}.testTag(tag).padding(horizontal = 12.dp, vertical = 10.dp), verticalAlignment = Alignment.CenterVertically) {
            Icon(icon, null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(22.dp))
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                Text(name, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                sub?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
            }
            Spacer(Modifier.width(8.dp))
            trailing()
        }
    }
}

@Composable
private fun StateChip(label: String, tone: ChipTone, progress: Float?) {
    val (bg, fg) = when (tone) {
        ChipTone.Good -> MaterialTheme.colorScheme.secondaryContainer to MaterialTheme.colorScheme.onSecondaryContainer
        ChipTone.Bad -> MaterialTheme.colorScheme.errorContainer to MaterialTheme.colorScheme.onErrorContainer
        ChipTone.Neutral -> MaterialTheme.colorScheme.surfaceContainer to MaterialTheme.colorScheme.onSurfaceVariant
    }
    Surface(shape = RoundedCornerShape(50), color = bg, contentColor = fg) {
        Row(Modifier.padding(horizontal = 10.dp, vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            if (progress != null) {
                CircularProgressIndicator(progress = { progress }, modifier = Modifier.size(12.dp), strokeWidth = 2.dp, color = fg, trackColor = fg.copy(alpha = 0.24f))
                Spacer(Modifier.width(6.dp))
            } else if (tone == ChipTone.Good) {
                Icon(HocketIcons.Filled.Check, null, Modifier.size(14.dp))
                Spacer(Modifier.width(4.dp))
            }
            Text(label, style = MaterialTheme.typography.labelMedium)
        }
    }
}

/** A titled card of monospace text that scrolls sideways rather than wrapping log lines. */
@Composable
private fun Block(title: String, text: String, tag: String, note: String? = null) {
    Surface(shape = RoundedCornerShape(24.dp), color = MaterialTheme.colorScheme.surfaceContainer, modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp)) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(title, style = MaterialTheme.typography.titleMedium, modifier = Modifier.semantics { heading() })
            note?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
            Surface(shape = RoundedCornerShape(12.dp), color = MaterialTheme.colorScheme.surfaceContainerHighest, modifier = Modifier.fillMaxWidth()) {
                SelectionContainer {
                    Text(
                        text,
                        fontFamily = FontFamily.Monospace,
                        fontSize = 11.sp,
                        lineHeight = 15.sp,
                        softWrap = false,
                        modifier = Modifier.horizontalScroll(rememberScrollState()).padding(12.dp).testTag(tag),
                    )
                }
            }
        }
    }
}
