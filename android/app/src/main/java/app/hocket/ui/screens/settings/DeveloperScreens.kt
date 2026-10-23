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
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
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
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.navigation.NavHostController
import app.hocket.R
import app.hocket.core.SettingKeys
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
 * Share (the share sheet, as a text file), Save (a document the user picks) and Copy. Every action
 * collects a fresh report, with the other devices' in the Connect room when that is switched on
 * (developer mode only). Reached from Advanced, or by shaking the phone in developer mode.
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
    var report by remember { mutableStateOf<DiagnosticsReport?>(null) }
    var refreshes by remember { mutableIntStateOf(0) }
    var busy by remember { mutableStateOf(false) }
    var allDevices by remember { mutableStateOf(false) }
    // Other devices only ever answer while developer mode is on here too.
    val withOthers = (devMode.bool ?: false) && allDevices
    LaunchedEffect(refreshes) {
        report = try {
            collect(context, client)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            null
        }
    }

    val notify: (String) -> Unit = { message -> snackbar?.let { s -> s.launch { s.host.showSnackbar(message) } } }
    val savedText = stringResource(R.string.developer_saved)
    val failedText = stringResource(R.string.developer_save_failed)
    val copiedText = stringResource(R.string.developer_copied)
    val copiedTruncatedText = stringResource(R.string.developer_copied_truncated)
    val copyFailedText = stringResource(R.string.developer_copy_failed)
    val shareTitle = stringResource(R.string.developer_share_title)
    /** Collects a fresh report, runs [action] on it, then refreshes the preview to match. */
    fun withFresh(action: suspend (DiagnosticsReport) -> Unit) {
        if (busy) return
        busy = true
        scope.launch {
            try {
                // Let the previous report go first: two of them (and the bundle text) at once is a lot
                // of memory with a big debug log.
                report = null
                val here = collect(context, client)
                report = here
                val fresh = if (withOthers) here.copy(otherDevices = collectOthers(client)) else here
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
        val on = devMode.bool ?: false
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
        if (on) {
            Surface(
                shape = RoundedCornerShape(24.dp),
                color = MaterialTheme.colorScheme.surfaceContainer,
                modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp).testTag("developer.allDevicesCard"),
            ) {
                Row(Modifier.padding(horizontal = 16.dp, vertical = 14.dp), verticalAlignment = Alignment.CenterVertically) {
                    Icon(HocketIcons.Filled.Devices, null, tint = MaterialTheme.colorScheme.primary)
                    Spacer(Modifier.width(14.dp))
                    Column(Modifier.weight(1f)) {
                        Text(stringResource(R.string.developer_all_devices), style = MaterialTheme.typography.bodyMedium)
                        Text(stringResource(R.string.developer_all_devices_body), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                    Spacer(Modifier.width(12.dp))
                    Switch(checked = allDevices, onCheckedChange = { allDevices = it }, modifier = Modifier.testTag("developer.allDevicesSwitch"))
                }
            }
        }
        FlowRow(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Button(onClick = { withFresh { DiagnosticsBundle.share(context, it, shareTitle) } }, enabled = !busy, modifier = Modifier.testTag("developer.share")) {
                Icon(HocketIcons.Filled.Share, null, Modifier.size(18.dp)); Spacer(Modifier.width(8.dp)); Text(stringResource(R.string.developer_share))
            }
            FilledTonalButton(onClick = { withFresh { pendingSave = it; saveLauncher.launch(it.fileName) } }, enabled = !busy, modifier = Modifier.testTag("developer.save")) {
                Icon(HocketIcons.Filled.Save, null, Modifier.size(18.dp)); Spacer(Modifier.width(8.dp)); Text(stringResource(R.string.developer_save))
            }
            FilledTonalButton(onClick = {
                withFresh {
                    val (clip, truncated) = withContext(Dispatchers.Default) {
                        val text = it.text
                        if (text.length > CLIPBOARD_MAX_CHARS) (it.head + "\n…\n" + text.takeLast(CLIPBOARD_MAX_CHARS - it.head.length - 3)) to true else text to false
                    }
                    val copied = runCatching { (context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager).setPrimaryClip(ClipData.newPlainText(it.fileName, clip)) }.isSuccess
                    notify(if (!copied) copyFailedText else if (truncated) copiedTruncatedText else copiedText)
                }
            }, enabled = !busy, modifier = Modifier.testTag("developer.copy")) {
                Icon(HocketIcons.Filled.ContentCopy, null, Modifier.size(18.dp)); Spacer(Modifier.width(8.dp)); Text(stringResource(R.string.developer_copy))
            }
            OutlinedButton(onClick = { refreshes++ }, enabled = !busy, modifier = Modifier.testTag("developer.refresh")) {
                Icon(HocketIcons.Filled.Refresh, null, Modifier.size(18.dp)); Spacer(Modifier.width(8.dp)); Text(stringResource(R.string.developer_refresh))
            }
        }
        if (busy && withOthers) {
            Text(
                stringResource(R.string.developer_collecting),
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
        Block(stringResource(R.string.developer_environment), r?.head?.trimEnd() ?: stringResource(R.string.developer_loading), "developer.head")
        Block(
            stringResource(R.string.developer_log),
            r?.let { it.preview.ifEmpty { stringResource(R.string.developer_empty) } } ?: stringResource(R.string.developer_loading),
            "developer.log",
            note = r?.let { stringResource(R.string.developer_log_tail, DiagnosticsReport.PREVIEW_LINES) },
        )
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
