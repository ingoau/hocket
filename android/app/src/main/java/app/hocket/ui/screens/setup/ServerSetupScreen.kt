package app.hocket.ui.screens.setup

import androidx.compose.animation.core.animateDpAsState
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.em
import app.hocket.ui.brand.AppTile
import app.hocket.ui.brand.Brand
import app.hocket.ui.brand.brandInk
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
import app.hocket.ui.icons.HocketIcons

/** Where setup is: the welcome, the sign-in form, or the summary of the server just connected. */
private enum class SetupStep { Welcome, Form, Summary }

/**
 * Server setup is the entire first screen (design: empty states, the Setup board): a welcome (the
 * tile and word mark, what Hocket needs, Connect to Navidrome); then the sign-in form on a sheet
 * under the shrunken mark (URL, username and password, connect), with a prominent warning for
 * `http://` addresses (the app allows cleartext, see the manifest); then the summary of the server
 * (its version, who is signed in where, its capabilities as chips, the first sync's progress) with
 * Continue ([onContinue]), or the clear error for servers below 0.63.0. [needsRelogin]: a login is
 * stored but unreadable right now; the form says so instead of looking like a sign-out.
 * [summary]: show the summary for [existing] (signed in, or too old) rather than the form. [onConnecting] runs when the form signs in, so the app holds setup open for the summary.
 */
@Composable
fun ServerSetupScreen(existing: ServerInfo?, needsRelogin: Boolean = false, summary: Boolean = existing != null && !needsRelogin, onConnecting: () -> Unit = {}, onContinue: () -> Unit = {}) {
    val client = LocalCoreClient.current
    var url by rememberSaveable { mutableStateOf(existing?.url ?: "") }
    var username by rememberSaveable { mutableStateOf(existing?.username ?: "") }
    // Never rememberSaveable: the saved-instance Bundle goes to system_server and outlives the process.
    var password by remember { mutableStateOf("") }
    var showPassword by rememberSaveable { mutableStateOf(false) }
    var connecting by rememberSaveable { mutableStateOf(false) }
    var urlError by rememberSaveable { mutableStateOf(false) }
    var error by rememberSaveable { mutableStateOf<String?>(null) }
    var started by rememberSaveable { mutableStateOf(needsRelogin) }
    val sync by client.syncProgress.collectAsStateWithLifecycle()

    LaunchedEffect(client) { client.errors.collect { e -> connecting = false; error = e.detail?.let { "${e.message}\n$it" } ?: e.message } }
    LaunchedEffect(existing) { if (existing != null) connecting = false }

    val step = when {
        existing != null && summary -> SetupStep.Summary
        started -> SetupStep.Form
        else -> SetupStep.Welcome
    }
    val valid = url.startsWith("http://") || url.startsWith("https://")
    fun connect() {
        urlError = !valid
        if (!valid || username.isBlank()) return
        error = null
        connecting = true
        onConnecting()
        val cleanUrl = url.trim().trimEnd('/')
        // The core never keeps the password: it lives in the platform keystore and is replayed on
        // every start. CoreHost stores it only once the core reports the server reachable with it.
        CoreHost.login(client::dispatch, ServerCredential(cleanUrl, username.trim(), password, null))
    }
    val cleartext = url.trim().startsWith("http://")
    val ground = Brush.verticalGradient(listOf(MaterialTheme.colorScheme.surface, MaterialTheme.colorScheme.surfaceContainer))

    BoxWithConstraints(Modifier.fillMaxSize().background(ground)) {
        val tooOld = existing != null && !existing.capabilities.meetsFloor
        // The mark: centred and large on the welcome, small over the form, mid-size over the summary.
        val tile by animateDpAsState(when (step) { SetupStep.Welcome -> 132.dp; SetupStep.Form -> 72.dp; SetupStep.Summary -> 104.dp }, label = "setupTile")
        val mark by animateDpAsState(when (step) { SetupStep.Welcome -> 52.dp; SetupStep.Form -> 26.dp; SetupStep.Summary -> 30.dp }, label = "setupMark")
        val top by animateDpAsState(when (step) { SetupStep.Welcome -> maxHeight * 0.30f; SetupStep.Form -> 40.dp; SetupStep.Summary -> 72.dp }, label = "setupTop")
        Column(Modifier.fillMaxWidth().systemBarsPadding().padding(top = top).alpha(if (tooOld) 0.45f else 1f), horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(if (step == SetupStep.Welcome) 28.dp else 14.dp)) {
            AppTile(tile)
            Wordmark(mark)
        }
        when (step) {
            SetupStep.Welcome -> Column(Modifier.align(Alignment.BottomCenter).fillMaxWidth().widthIn(max = 480.dp).systemBarsPadding().padding(start = 24.dp, end = 24.dp, bottom = 40.dp), verticalArrangement = Arrangement.spacedBy(20.dp)) {
                Text(stringResource(R.string.setup_subtitle), style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = TextAlign.Center, modifier = Modifier.fillMaxWidth())
                Button(onClick = { started = true }, shapes = ButtonDefaults.shapes(), modifier = Modifier.fillMaxWidth().heightIn(min = ButtonDefaults.LargeContainerHeight).testTag("setup.start"),
                    contentPadding = ButtonDefaults.contentPaddingFor(ButtonDefaults.LargeContainerHeight)) {
                    Text(stringResource(R.string.setup_title), style = ButtonDefaults.textStyleFor(ButtonDefaults.MediumContainerHeight))
                }
            }
            SetupStep.Form -> Surface(
                color = MaterialTheme.colorScheme.surfaceContainer, shape = RoundedCornerShape(topStart = 28.dp, topEnd = 28.dp),
                modifier = Modifier.align(Alignment.BottomCenter).fillMaxWidth().heightIn(max = maxHeight - 170.dp),
            ) {
                Column(Modifier.fillMaxWidth().navigationBarsPadding().imePadding().verticalScroll(rememberScrollState()).padding(start = 24.dp, end = 24.dp, top = 28.dp, bottom = 24.dp)) {
                    Text(stringResource(R.string.setup_title), style = MaterialTheme.typography.headlineMedium, fontWeight = FontWeight.SemiBold, modifier = Modifier.semantics { heading() })
                    Text(stringResource(R.string.setup_subtitle), style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(top = 4.dp, bottom = 20.dp))
                    if (needsRelogin) {
                        Surface(color = MaterialTheme.colorScheme.secondaryContainer, shape = RoundedCornerShape(18.dp), modifier = Modifier.fillMaxWidth().padding(bottom = 16.dp).testTag("setup.relogin")) {
                            Text(stringResource(R.string.setup_relogin_notice), color = MaterialTheme.colorScheme.onSecondaryContainer, modifier = Modifier.padding(horizontal = 14.dp, vertical = 12.dp), style = MaterialTheme.typography.bodyMedium)
                        }
                    }
                    val field = RoundedCornerShape(16.dp)
                    OutlinedTextField(
                        value = url, onValueChange = { url = it; urlError = false }, label = { Text(stringResource(R.string.setup_url)) },
                        placeholder = { Text(stringResource(R.string.setup_url_hint)) }, singleLine = true, isError = urlError, shape = field,
                        supportingText = if (urlError) ({ Text(stringResource(R.string.setup_error_url)) }) else null,
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Next),
                        modifier = Modifier.fillMaxWidth().testTag("setup.url"), enabled = !connecting,
                    )
                    if (cleartext) {
                        Surface(color = MaterialTheme.colorScheme.errorContainer, shape = RoundedCornerShape(18.dp), modifier = Modifier.fillMaxWidth().padding(top = 8.dp).testTag("setup.cleartextWarning")) {
                            Text(stringResource(R.string.setup_cleartext_warning), color = MaterialTheme.colorScheme.onErrorContainer, modifier = Modifier.padding(horizontal = 14.dp, vertical = 12.dp), style = MaterialTheme.typography.bodyMedium)
                        }
                    }
                    val user = @Composable { m: Modifier ->
                        OutlinedTextField(value = username, onValueChange = { username = it }, label = { Text(stringResource(R.string.setup_username)) }, singleLine = true, shape = field,
                            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Next), modifier = m.testTag("setup.username"), enabled = !connecting)
                    }
                    val pass = @Composable { m: Modifier ->
                        OutlinedTextField(
                            value = password, onValueChange = { password = it }, label = { Text(stringResource(R.string.setup_password)) }, singleLine = true, shape = field,
                            visualTransformation = if (showPassword) VisualTransformation.None else PasswordVisualTransformation(),
                            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password, imeAction = ImeAction.Go),
                            keyboardActions = KeyboardActions(onGo = { connect() }),
                            trailingIcon = {
                                IconButton(onClick = { showPassword = !showPassword }) {
                                    Icon(if (showPassword) HocketIcons.Filled.VisibilityOff else HocketIcons.Filled.Visibility, stringResource(if (showPassword) R.string.setup_hide_password else R.string.setup_show_password))
                                }
                            },
                            modifier = m.testTag("setup.password"), enabled = !connecting,
                        )
                    }
                    // Side by side as on the board; stacked when the text is large.
                    if (LocalDensity.current.fontScale < 1.3f) Row(Modifier.padding(top = 14.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) { user(Modifier.weight(1f)); pass(Modifier.weight(1f)) }
                    else Column(Modifier.padding(top = 14.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) { user(Modifier.fillMaxWidth()); pass(Modifier.fillMaxWidth()) }
                    error?.let {
                        Surface(color = MaterialTheme.colorScheme.errorContainer, shape = RoundedCornerShape(18.dp), modifier = Modifier.fillMaxWidth().padding(top = 16.dp).testTag("setup.error")) {
                            Text(it, color = MaterialTheme.colorScheme.onErrorContainer, modifier = Modifier.padding(horizontal = 14.dp, vertical = 12.dp), style = MaterialTheme.typography.bodyMedium)
                        }
                    }
                    Spacer(Modifier.height(28.dp))
                    Button(onClick = ::connect, enabled = !connecting && url.isNotBlank() && username.isNotBlank(), shapes = ButtonDefaults.shapes(),
                        modifier = Modifier.fillMaxWidth().heightIn(min = ButtonDefaults.LargeContainerHeight).testTag("setup.connect"), contentPadding = ButtonDefaults.contentPaddingFor(ButtonDefaults.LargeContainerHeight)) {
                        if (connecting) {
                            LoadingIndicator(modifier = Modifier.size(24.dp), color = MaterialTheme.colorScheme.onPrimary)
                            Spacer(Modifier.size(12.dp))
                            Text(stringResource(R.string.setup_connecting), style = ButtonDefaults.textStyleFor(ButtonDefaults.MediumContainerHeight))
                        } else Text(stringResource(R.string.setup_connect), style = ButtonDefaults.textStyleFor(ButtonDefaults.MediumContainerHeight))
                    }
                }
            }
            SetupStep.Summary -> existing?.let { server ->
                Column(
                    Modifier.fillMaxSize().systemBarsPadding().verticalScroll(rememberScrollState()).padding(start = 24.dp, end = 24.dp, top = 72.dp + 104.dp + 14.dp + 48.dp, bottom = 32.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    ServerSummary(server, sync, onContinue = onContinue, onDifferentServer = { CoreHost.removeServer(client::dispatch, server); started = true })
                }
            }
        }
    }
}

/** "hocket" in the word mark's face at [size], the brand's ink. */
@Composable
private fun Wordmark(size: androidx.compose.ui.unit.Dp) {
    val sp = with(LocalDensity.current) { size.toSp() }
    Text("hocket", color = brandInk(), style = androidx.compose.ui.text.TextStyle(fontFamily = Brand.unbounded, fontWeight = FontWeight.Medium, fontSize = sp, letterSpacing = (-0.03).em),
        modifier = Modifier.semantics { heading() })
}

/**
 * The server just connected: its version (in the error colour when too old), who is signed in
 * where, the capabilities as chips, the first sync as a split progress bar, and Continue; a server
 * below the floor says why and offers only Use a different server.
 */
@Composable
private fun ServerSummary(server: ServerInfo, sync: app.hocket.core.api.SyncProgress?, onContinue: () -> Unit, onDifferentServer: () -> Unit) {
    val caps = server.capabilities
    val host = server.url.removePrefix("https://").removePrefix("http://").trimEnd('/')
    Column(Modifier.fillMaxWidth().widthIn(max = 480.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        Text(stringResource(R.string.setup_caps_version, caps.serverVersion ?: "?"), style = MaterialTheme.typography.headlineMedium, fontWeight = FontWeight.SemiBold,
            color = if (caps.meetsFloor) MaterialTheme.colorScheme.onSurface else MaterialTheme.colorScheme.error, textAlign = TextAlign.Center)
        Text(stringResource(R.string.setup_who, server.username, host), style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = TextAlign.Center, modifier = Modifier.padding(top = 4.dp))
        if (!caps.meetsFloor) {
            Surface(color = MaterialTheme.colorScheme.errorContainer, shape = RoundedCornerShape(20.dp), modifier = Modifier.fillMaxWidth().padding(top = 20.dp).testTag("setup.tooOld")) {
                Text(stringResource(R.string.setup_too_old, caps.serverVersion ?: "?"), color = MaterialTheme.colorScheme.onErrorContainer, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(horizontal = 16.dp, vertical = 14.dp))
            }
        }
        FlowRow(Modifier.padding(top = 24.dp), horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.CenterHorizontally), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            CapChip(stringResource(R.string.setup_caps_lyrics), caps.songLyrics)
            CapChip(stringResource(R.string.setup_caps_sonic), caps.sonicSimilarity)
            CapChip(stringResource(R.string.setup_caps_native), caps.nativeApi)
        }
        if (caps.meetsFloor) {
            // This server's sync only: after "Use a different server" the old one's finished run doesn't count.
            val s = sync?.takeIf { it.serverId == server.id }
            Text(when { s == null -> stringResource(R.string.setup_sync_starting); s.finished -> stringResource(R.string.setup_synced); else -> stringResource(R.string.setup_syncing, s.phase) }, style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(top = 28.dp).semantics { liveRegion = LiveRegionMode.Polite })
            val total = s?.total?.toFloat()?.takeIf { it > 0f }
            val progress = when { s == null -> null; s.finished -> 1f; total != null -> s.done.toFloat() / total; else -> null }
            if (progress != null) LinearProgressIndicator(progress = { progress }, modifier = Modifier.fillMaxWidth().padding(top = 10.dp).height(6.dp))
            else LinearProgressIndicator(modifier = Modifier.fillMaxWidth().padding(top = 10.dp).height(6.dp))
            Spacer(Modifier.height(40.dp))
            Button(onClick = onContinue, shapes = ButtonDefaults.shapes(), modifier = Modifier.fillMaxWidth().heightIn(min = ButtonDefaults.LargeContainerHeight).testTag("setup.continue"),
                contentPadding = ButtonDefaults.contentPaddingFor(ButtonDefaults.LargeContainerHeight)) { Text(stringResource(R.string.setup_continue), style = ButtonDefaults.textStyleFor(ButtonDefaults.MediumContainerHeight)) }
            TextButton(onClick = onDifferentServer, modifier = Modifier.padding(top = 8.dp).heightIn(min = 48.dp).testTag("setup.differentServer")) { Text(stringResource(R.string.setup_remove_server)) }
        } else {
            Spacer(Modifier.height(40.dp))
            Button(onClick = onDifferentServer, shapes = ButtonDefaults.shapes(), modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp).testTag("setup.differentServer")) { Text(stringResource(R.string.setup_remove_server)) }
        }
    }
}

/** A capability as a chip: filled with a check when the server has it, outlined with a cross when not. */
@Composable
private fun CapChip(label: String, ok: Boolean) {
    val state = stringResource(if (ok) R.string.cap_yes else R.string.cap_no)
    Surface(shape = CircleShape, color = if (ok) MaterialTheme.colorScheme.secondaryContainer else Color.Transparent,
        border = if (ok) null else androidx.compose.foundation.BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
        modifier = Modifier.semantics(mergeDescendants = true) { contentDescription = "$label, $state" }) {
        Row(Modifier.heightIn(min = 40.dp).padding(start = 10.dp, end = 14.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Icon(if (ok) HocketIcons.Filled.Check else HocketIcons.Filled.Close, null, Modifier.size(18.dp), tint = if (ok) MaterialTheme.colorScheme.onSecondaryContainer else MaterialTheme.colorScheme.onSurfaceVariant)
            Text(label, style = MaterialTheme.typography.labelLarge, color = if (ok) MaterialTheme.colorScheme.onSecondaryContainer else MaterialTheme.colorScheme.onSurfaceVariant)
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
            Text(stringResource(R.string.setup_too_old, caps.serverVersion ?: "?"), color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodyMedium)
        }
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            CapChip(stringResource(R.string.setup_caps_lyrics), caps.songLyrics)
            CapChip(stringResource(R.string.setup_caps_sonic), caps.sonicSimilarity)
            CapChip(stringResource(R.string.setup_caps_native), caps.nativeApi)
        }
    }
}
