package app.hocket.ui.screens.setup

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.systemBarsPadding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Visibility
import androidx.compose.material.icons.filled.VisibilityOff
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearWavyProgressIndicator
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.R
import app.hocket.core.api.ServerInfo
import app.hocket.playback.CoreHost
import app.hocket.playback.ServerCredential
import app.hocket.ui.LocalCoreClient

/**
 * Server setup is the entire first screen (design: empty states). URL, username, password, connect.
 * Progress while the core probes and starts the sync; the capability summary and a clear error for
 * servers below 0.63.0. A prominent warning for `http://` addresses (the app allows cleartext, see
 * the manifest). [needsRelogin]: a login is stored but unreadable right now; say so instead of
 * looking like a sign-out.
 */
@Composable
fun ServerSetupScreen(existing: ServerInfo?, needsRelogin: Boolean = false) {
    val client = LocalCoreClient.current
    var url by rememberSaveable { mutableStateOf(existing?.url ?: "") }
    var username by rememberSaveable { mutableStateOf(existing?.username ?: "") }
    // Never rememberSaveable: the saved-instance Bundle goes to system_server and outlives the process.
    var password by remember { mutableStateOf("") }
    var showPassword by rememberSaveable { mutableStateOf(false) }
    var connecting by rememberSaveable { mutableStateOf(false) }
    var urlError by rememberSaveable { mutableStateOf(false) }
    var error by rememberSaveable { mutableStateOf<String?>(null) }
    val sync by client.syncProgress.collectAsStateWithLifecycle()

    LaunchedEffect(client) { client.errors.collect { e -> connecting = false; error = e.detail?.let { "${e.message}\n$it" } ?: e.message } }
    LaunchedEffect(existing) { if (existing != null) connecting = false }

    val valid = url.startsWith("http://") || url.startsWith("https://")
    fun connect() {
        urlError = !valid
        if (!valid || username.isBlank()) return
        error = null
        connecting = true
        val cleanUrl = url.trim().trimEnd('/')
        // The core never keeps the password: it lives in the platform keystore and is replayed on
        // every start. CoreHost stores it only once the core reports the server reachable with it.
        CoreHost.login(client::dispatch, ServerCredential(cleanUrl, username.trim(), password, null))
    }
    val cleartext = url.trim().startsWith("http://")

    Surface(Modifier.fillMaxSize()) {
        Column(
            Modifier.fillMaxSize().systemBarsPadding().imePadding().verticalScroll(rememberScrollState()).padding(24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Spacer(Modifier.height(48.dp))
            Column(Modifier.widthIn(max = 480.dp)) {
                Text(stringResource(R.string.setup_title), style = MaterialTheme.typography.displaySmall)
                Spacer(Modifier.height(8.dp))
                Text(stringResource(R.string.setup_subtitle), style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
                if (needsRelogin) {
                    Spacer(Modifier.height(16.dp))
                    Surface(color = MaterialTheme.colorScheme.secondaryContainer, shape = MaterialTheme.shapes.medium, modifier = Modifier.fillMaxWidth().testTag("setup.relogin")) {
                        Text(stringResource(R.string.setup_relogin_notice), color = MaterialTheme.colorScheme.onSecondaryContainer, modifier = Modifier.padding(16.dp), style = MaterialTheme.typography.bodyMedium)
                    }
                }
                Spacer(Modifier.height(32.dp))
                OutlinedTextField(
                    value = url, onValueChange = { url = it; urlError = false }, label = { Text(stringResource(R.string.setup_url)) },
                    placeholder = { Text(stringResource(R.string.setup_url_hint)) }, singleLine = true, isError = urlError,
                    supportingText = if (urlError) ({ Text(stringResource(R.string.setup_error_url)) }) else null,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Next),
                    modifier = Modifier.fillMaxWidth().testTag("setup.url"), enabled = !connecting,
                )
                if (cleartext) {
                    Spacer(Modifier.height(8.dp))
                    Surface(color = MaterialTheme.colorScheme.errorContainer, shape = MaterialTheme.shapes.medium, modifier = Modifier.fillMaxWidth().testTag("setup.cleartextWarning")) {
                        Text(stringResource(R.string.setup_cleartext_warning), color = MaterialTheme.colorScheme.onErrorContainer, modifier = Modifier.padding(16.dp), style = MaterialTheme.typography.bodyMedium)
                    }
                }
                Spacer(Modifier.height(12.dp))
                OutlinedTextField(
                    value = username, onValueChange = { username = it }, label = { Text(stringResource(R.string.setup_username)) }, singleLine = true,
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Next), modifier = Modifier.fillMaxWidth().testTag("setup.username"), enabled = !connecting,
                )
                Spacer(Modifier.height(12.dp))
                OutlinedTextField(
                    value = password, onValueChange = { password = it }, label = { Text(stringResource(R.string.setup_password)) }, singleLine = true,
                    visualTransformation = if (showPassword) VisualTransformation.None else PasswordVisualTransformation(),
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password, imeAction = ImeAction.Go),
                    keyboardActions = KeyboardActions(onGo = { connect() }),
                    trailingIcon = {
                        IconButton(onClick = { showPassword = !showPassword }) {
                            Icon(if (showPassword) Icons.Filled.VisibilityOff else Icons.Filled.Visibility, stringResource(if (showPassword) R.string.setup_hide_password else R.string.setup_show_password))
                        }
                    },
                    modifier = Modifier.fillMaxWidth().testTag("setup.password"), enabled = !connecting,
                )
                Spacer(Modifier.height(24.dp))
                Button(onClick = ::connect, enabled = !connecting && url.isNotBlank() && username.isNotBlank(), shapes = ButtonDefaults.shapes(), modifier = Modifier.fillMaxWidth().height(56.dp).testTag("setup.connect")) {
                    if (connecting) {
                        LoadingIndicator(modifier = Modifier.size(24.dp), color = MaterialTheme.colorScheme.onPrimary)
                        Spacer(Modifier.size(12.dp))
                        Text(stringResource(R.string.setup_connecting))
                    } else Text(stringResource(R.string.setup_connect))
                }
                error?.let {
                    Spacer(Modifier.height(16.dp))
                    Surface(color = MaterialTheme.colorScheme.errorContainer, shape = MaterialTheme.shapes.medium, modifier = Modifier.fillMaxWidth().testTag("setup.error")) {
                        Text(it, color = MaterialTheme.colorScheme.onErrorContainer, modifier = Modifier.padding(16.dp), style = MaterialTheme.typography.bodyMedium)
                    }
                }
                existing?.let { server ->
                    Spacer(Modifier.height(24.dp))
                    CapabilitySummary(server)
                    sync?.takeIf { !it.finished }?.let { s ->
                        Spacer(Modifier.height(12.dp))
                        Text(stringResource(R.string.setup_syncing, s.phase), style = MaterialTheme.typography.bodyMedium)
                        val total = s.total?.toFloat()?.takeIf { it > 0f }
                        if (total != null) LinearWavyProgressIndicator(progress = { s.done.toFloat() / total }, modifier = Modifier.fillMaxWidth().padding(top = 8.dp))
                        else LinearWavyProgressIndicator(modifier = Modifier.fillMaxWidth().padding(top = 8.dp))
                    }
                    Spacer(Modifier.height(12.dp))
                    TextButton(onClick = { CoreHost.removeServer(client::dispatch, server) }) { Text(stringResource(R.string.setup_remove_server)) }
                }
            }
        }
    }
}

@Composable
fun CapabilitySummary(server: ServerInfo) {
    val caps = server.capabilities
    Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Text(stringResource(R.string.setup_caps_title), style = MaterialTheme.typography.titleMedium)
        Text(stringResource(R.string.setup_caps_version, caps.serverVersion ?: "?"), style = MaterialTheme.typography.bodyMedium,
            color = if (caps.meetsFloor) MaterialTheme.colorScheme.onSurface else MaterialTheme.colorScheme.error)
        if (!caps.meetsFloor) {
            Text(stringResource(R.string.setup_too_old, caps.serverVersion ?: "?"), color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.testTag("setup.tooOld"))
        }
        CapRow(stringResource(R.string.setup_caps_lyrics), caps.songLyrics)
        CapRow(stringResource(R.string.setup_caps_sonic), caps.sonicSimilarity)
        CapRow(stringResource(R.string.setup_caps_transcoding), caps.transcodingExtension)
        CapRow(stringResource(R.string.setup_caps_native), caps.nativeApi)
        CapRow(stringResource(R.string.setup_caps_apikey), caps.apiKeyAuthentication)
    }
}

@Composable
private fun CapRow(label: String, ok: Boolean) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Icon(if (ok) Icons.Filled.Check else Icons.Filled.Close, stringResource(if (ok) R.string.cap_yes else R.string.cap_no), tint = if (ok) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.outline, modifier = Modifier.size(18.dp))
        Spacer(Modifier.size(8.dp))
        Text(label, style = MaterialTheme.typography.bodyMedium)
    }
}
