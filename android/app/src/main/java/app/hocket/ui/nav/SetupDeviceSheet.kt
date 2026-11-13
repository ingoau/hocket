package app.hocket.ui.nav

import android.content.Intent
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.R
import app.hocket.core.HocketJson
import app.hocket.core.api.Query
import app.hocket.core.api.QueryResult
import app.hocket.core.api.ServerInfo
import app.hocket.playback.CoreHost
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.icons.HocketIcons
import app.hocket.ui.screens.setup.SetupLink
import com.google.zxing.BarcodeFormat
import com.google.zxing.EncodeHintType
import com.google.zxing.common.BitMatrix
import com.google.zxing.qrcode.QRCodeWriter
import com.google.zxing.qrcode.decoder.ErrorCorrectionLevel
import kotlinx.serialization.json.jsonObject

/**
 * Set up another device, from the account sheet: a QR code of this device's [SetupLink] (the
 * server, who is signed in, the coordinator when there is one, and the password only when the
 * person switches it on), which Hocket's setup scanner or a phone camera opens on the other
 * device; Share link sends the same link (to a computer, say). Off by default, the password is
 * read from the keystore only while it is on.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SetupDeviceSheet(onDismiss: () -> Unit) {
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true), modifier = Modifier.testTag("setupDevice.sheet")) {
        val client = LocalCoreClient.current
        val server by client.server.collectAsStateWithLifecycle()
        val connection by client.connection.collectAsStateWithLifecycle()
        val coordinatorUrl = connection.coordinatorUrl?.takeIf { it.isNotBlank() }
        // Without a coordinator the settings ride in the link; null while the core answers.
        val settings by produceState<String?>(null, coordinatorUrl) {
            value = if (coordinatorUrl != null) null else (client.query(Query.SetupSettings) as? QueryResult.Text)?.data
        }
        server?.let { SetupDeviceSheetContent(it, coordinatorUrl, settings) }
    }
}

/** The longest link the code carries; past it the settings stay out of the code (Share link keeps them). */
internal const val MAX_CODE_LINK = 1800

/**
 * The sheet's content for [server] (also drawn on its own by tests). [settings] is
 * `Query.SetupSettings` (a JSON object), for a device without a coordinator.
 */
@Composable
fun SetupDeviceSheetContent(server: ServerInfo, coordinatorUrl: String?, settings: String? = null) {
    val context = LocalContext.current
    var includePassword by remember { mutableStateOf(false) }
    // The keystore read happens only once the person asks for the password to go in.
    val password = remember(server.url, server.username, includePassword) {
        if (includePassword) CoreHost.credentials?.get(server.url, server.username)?.password else null
    }
    val passwordAvailable = remember(server.url, server.username) { CoreHost.credentials?.get(server.url, server.username) != null }
    val coordinator = coordinatorUrl?.takeIf { it.isNotBlank() }
    val settingsCount = remember(settings) { settings?.let { runCatching { HocketJson.json.parseToJsonElement(it).jsonObject.size }.getOrNull() } ?: 0 }
    val carried = settings?.takeIf { coordinator == null && settingsCount > 0 }
    val link = SetupLink(server.url, server.username, password, coordinator, carried).format()
    // A code past this many characters gets too dense to scan off a screen: it leaves the settings out.
    val codeLink = if (link.length <= MAX_CODE_LINK) link else SetupLink(server.url, server.username, password, coordinator).format()
    val host = server.url.removePrefix("https://").removePrefix("http://")

    Column(Modifier.verticalScroll(rememberScrollState()).navigationBarsPadding().padding(start = 24.dp, end = 24.dp, bottom = 24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        Text(stringResource(R.string.setup_device_title), style = MaterialTheme.typography.titleLarge, modifier = Modifier.fillMaxWidth().semantics { heading() })
        Text(stringResource(R.string.setup_device_body), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.fillMaxWidth().padding(top = 4.dp, bottom = 20.dp))
        // Dark modules on white in either theme: scanners expect a light quiet zone around a dark code.
        val codeDescription = stringResource(R.string.setup_device_code, host)
        Box(
            Modifier.widthIn(max = 280.dp).fillMaxWidth().aspectRatio(1f).clip(RoundedCornerShape(24.dp)).background(Color.White).padding(20.dp)
                .semantics { contentDescription = codeDescription }.testTag("setupDevice.code"),
        ) {
            QrCode(codeLink, Modifier.fillMaxWidth().aspectRatio(1f))
        }
        Column(Modifier.fillMaxWidth().padding(top = 16.dp)) {
            InfoRow(stringResource(R.string.setup_device_server), stringResource(R.string.account_signed_in, server.username, host))
            InfoRow(stringResource(R.string.setup_device_coordinator), coordinator ?: stringResource(R.string.setup_device_no_coordinator), tag = "setupDevice.coordinator")
            InfoRow(
                stringResource(R.string.setup_device_settings),
                when {
                    coordinator != null -> stringResource(R.string.setup_device_settings_synced)
                    carried == null -> stringResource(R.string.setup_device_settings_none)
                    codeLink != link -> stringResource(R.string.setup_device_settings_too_many)
                    else -> pluralStringResource(R.plurals.setup_device_settings_count, settingsCount, settingsCount)
                },
                tag = "setupDevice.settings",
            )
            val on = includePassword && password != null
            ListItem(
                headlineContent = { Text(stringResource(R.string.setup_device_include_password)) },
                supportingContent = {
                    Text(stringResource(when {
                        !passwordAvailable -> R.string.setup_device_password_unavailable
                        on -> R.string.setup_device_include_password_on
                        else -> R.string.setup_device_include_password_off
                    }), color = if (on) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant)
                },
                trailingContent = { Switch(checked = on, onCheckedChange = null, enabled = passwordAvailable) },
                colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                modifier = Modifier.toggleable(value = on, enabled = passwordAvailable, role = Role.Switch) { includePassword = it }.testTag("setupDevice.includePassword"),
            )
        }
        OutlinedButton(
            onClick = {
                val send = Intent(Intent.ACTION_SEND).apply { type = "text/plain"; putExtra(Intent.EXTRA_TEXT, link) }
                context.startActivity(Intent.createChooser(send, null))
            },
            shapes = ButtonDefaults.shapes(), modifier = Modifier.padding(top = 8.dp).testTag("setupDevice.share"),
        ) {
            Icon(HocketIcons.Filled.Share, null, Modifier.size(ButtonDefaults.IconSize))
            Text(stringResource(R.string.setup_device_share), Modifier.padding(start = ButtonDefaults.IconSpacing))
        }
    }
}

@Composable
private fun InfoRow(label: String, value: String, tag: String? = null) {
    ListItem(
        overlineContent = { Text(label) },
        headlineContent = { Text(value) },
        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
        modifier = if (tag != null) Modifier.testTag(tag) else Modifier,
    )
}

/** [text] as a QR code, black modules filling the box (the caller draws the quiet zone). */
@Composable
private fun QrCode(text: String, modifier: Modifier) {
    val matrix = remember(text) { qrMatrix(text) }
    Canvas(modifier) {
        val cell = size.width / matrix.width
        for (y in 0 until matrix.height) for (x in 0 until matrix.width) {
            // A hair wider than a module, so neighbours meet without anti-aliased seams.
            if (matrix[x, y]) drawRect(Color.Black, Offset(x * cell, y * cell), Size(cell + 0.5f, cell + 0.5f))
        }
    }
}

/** The modules of [text]'s QR code, one cell per module and no margin. */
internal fun qrMatrix(text: String): BitMatrix =
    QRCodeWriter().encode(text, BarcodeFormat.QR_CODE, 0, 0, mapOf(EncodeHintType.ERROR_CORRECTION to ErrorCorrectionLevel.M, EncodeHintType.MARGIN to 0, EncodeHintType.CHARACTER_SET to "UTF-8"))
