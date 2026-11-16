package app.hocket.ui.screens.settings

import android.app.Activity
import android.text.format.DateUtils
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.R
import app.hocket.YtmSignInActivity
import app.hocket.core.api.Command
import app.hocket.core.api.CommandClaimCoordinatorInner
import app.hocket.core.api.CommandRedeemCoordinatorInviteInner
import app.hocket.core.api.CommandRemoveDeviceInner
import app.hocket.core.api.CommandRevokeCoordinatorInviteInner
import app.hocket.core.api.CommandSetCredentialSyncInner
import app.hocket.core.api.ConnectionTier
import app.hocket.core.api.IdentityDevice
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.nav.PairDeviceSheet
import app.hocket.ui.screens.setup.PairingLinks

/**
 * Settings → Connect's devices: the identity's devices (when each last synced, and Remove, which
 * says to change passwords), Add a device, credential sync (saying the coordinator holds the key
 * while it's on), and the coordinator: claim it with a `hocket-coordinator pair` code (typed or
 * scanned) or join one with an invite; once pinned, its address, whose it is, the connection to it
 * and Disconnect / Reconnect; and the owner's invites and its YouTube Music analysis sign-in.
 */
@Composable
fun ColumnScope.IdentitySettings() {
    val client = LocalCoreClient.current
    val identity by client.identity.collectAsStateWithLifecycle()
    var removing by remember { mutableStateOf<IdentityDevice?>(null) }
    var adding by rememberSaveable { mutableStateOf(false) }
    SettingsSection(stringResource(R.string.identity_devices))
    if (identity.removed) {
        SettingRow(stringResource(R.string.identity_removed_banner), trailing = {
            TextButton(onClick = { client.dispatch(Command.ResetIdentity) }) { Text(stringResource(R.string.identity_start_over)) }
        })
    }
    for (d in identity.devices) {
        val synced = d.lastSynced?.let { stringResource(R.string.identity_last_synced, DateUtils.getRelativeTimeSpanString(it.toLong()).toString()) }
            ?: stringResource(R.string.identity_never_synced)
        val title = if (d.isSelf) "${d.name} · ${stringResource(R.string.identity_this_device)}" else d.name
        SettingRow(title, synced, trailing = {
            TextButton(onClick = { removing = d }, modifier = Modifier.testTag("identity.remove.${d.id}")) { Text(stringResource(R.string.identity_remove)) }
        })
    }
    OutlinedButton(onClick = { adding = true }, shapes = ButtonDefaults.shapes(), modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp).testTag("identity.add")) { Text(stringResource(R.string.identity_add)) }
    val syncSubtitle = stringResource(
        when {
            !identity.credentialSync -> R.string.identity_credential_sync_off
            identity.coordinator != null -> R.string.identity_credential_sync_coordinator
            else -> R.string.identity_credential_sync_on
        },
    )
    SwitchRow(stringResource(R.string.identity_credential_sync), identity.credentialSync, { client.dispatch(Command.SetCredentialSync(CommandSetCredentialSyncInner(it))) }, subtitle = syncSubtitle)
    Coordinator()
    if (adding) PairDeviceSheet(onDismiss = { adding = false })
    removing?.let { d ->
        AlertDialog(
            onDismissRequest = { removing = null },
            title = { Text(stringResource(R.string.identity_remove_title, d.name)) },
            text = { Text(stringResource(if (d.isSelf) R.string.identity_remove_self_body else R.string.identity_remove_body)) },
            confirmButton = { TextButton(onClick = { removing = null; client.dispatch(Command.RemoveDevice(CommandRemoveDeviceInner(d.id))) }, modifier = Modifier.testTag("identity.removeConfirm")) { Text(stringResource(R.string.identity_remove)) } },
            dismissButton = { TextButton(onClick = { removing = null }) { Text(stringResource(R.string.action_cancel)) } },
        )
    }
}

@Composable
private fun ColumnScope.Coordinator() {
    val client = LocalCoreClient.current
    val context = LocalContext.current
    val identity by client.identity.collectAsStateWithLifecycle()
    val invites by client.coordinatorInvites.collectAsStateWithLifecycle()
    val connection by client.connection.collectAsStateWithLifecycle()
    var url by rememberSaveable { mutableStateOf("") }
    var code by rememberSaveable { mutableStateOf("") }
    val pin = identity.coordinator
    LaunchedEffect(pin?.owner, pin?.url) {
        if (pin?.owner == true) {
            client.dispatch(Command.ListCoordinatorInvites)
            client.dispatch(Command.CoordinatorYtmStatus)
        }
    }
    // The search's "coordinator" entry lands here.
    SettingsSection(stringResource(R.string.identity_coordinator), tag = "connect.coordinator")
    if (pin == null) {
        SettingRow(stringResource(R.string.identity_coordinator_none), tag = "identity.coordinatorNone")
        OutlinedTextField(value = url, onValueChange = { url = it }, label = { Text(stringResource(R.string.identity_coordinator_url)) }, placeholder = { Text(stringResource(R.string.identity_coordinator_url_hint)) }, singleLine = true, modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp).testTag("identity.coordinatorUrl"))
        OutlinedTextField(value = code, onValueChange = { code = it }, label = { Text(stringResource(R.string.identity_coordinator_code)) }, singleLine = true, modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp).testTag("identity.coordinatorCode"))
        Row(Modifier.padding(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Button(shapes = ButtonDefaults.shapes(), enabled = code.isNotBlank(), onClick = { client.dispatch(Command.ClaimCoordinator(CommandClaimCoordinatorInner(url.trim(), code.trim()))) }, modifier = Modifier.testTag("identity.claim")) { Text(stringResource(R.string.identity_claim)) }
            TextButton(enabled = code.isNotBlank() && url.isNotBlank(), onClick = { client.dispatch(Command.RedeemCoordinatorInvite(CommandRedeemCoordinatorInviteInner(url.trim(), code.trim()))) }, modifier = Modifier.testTag("identity.redeem")) { Text(stringResource(R.string.identity_redeem)) }
            if (app.hocket.SetupScanner.available) {
                TextButton(onClick = {
                    app.hocket.SetupScanner.scan(context, onResult = { text -> PairingLinks.claim(text)?.let { (u, c) -> if (u.isNotEmpty()) url = u; code = c } ?: run { code = text.trim() } }, onError = {})
                }) { Text(stringResource(R.string.identity_scan_claim)) }
            }
        }
    } else {
        // Pinned: where it is, whose it is, and how this device is connected to it.
        SettingRow(stringResource(if (pin.owner) R.string.identity_coordinator_owner else R.string.identity_coordinator_guest, pin.url), tag = "identity.coordinatorPin")
        val status = when (connection.tier) {
            ConnectionTier.Local -> stringResource(R.string.connection_local)
            ConnectionTier.Lan -> pluralStringResource(R.plurals.connection_lan, connection.peerCount.toInt(), connection.peerCount.toInt())
            ConnectionTier.Coordinator -> pluralStringResource(R.plurals.connection_coordinator, connection.peerCount.toInt(), connection.peerCount.toInt())
        }
        val connected = connection.connected && connection.tier == ConnectionTier.Coordinator
        SettingRow(status, connection.error ?: connection.roundTripMs?.let { "${it.toInt()} ms" }, tag = "identity.coordinatorStatus", trailing = {
            if (connected) {
                TextButton(onClick = { client.dispatch(Command.DisconnectCoordinator) }, modifier = Modifier.testTag("identity.coordinatorDisconnect")) { Text(stringResource(R.string.identity_coordinator_disconnect)) }
            } else {
                TextButton(onClick = { client.dispatch(Command.ConnectCoordinator) }, modifier = Modifier.testTag("identity.coordinatorReconnect")) { Text(stringResource(R.string.identity_coordinator_reconnect)) }
            }
        })
    }
    if (pin?.owner == true) {
        SettingsSection(stringResource(R.string.identity_invites))
        SettingRow(stringResource(R.string.identity_invites_hint))
        val list = invites?.invites.orEmpty()
        if (list.isEmpty()) SettingRow(stringResource(R.string.identity_invites_none))
        for (i in list) {
            val status = if (i.redeemed) stringResource(R.string.identity_invite_used)
                else stringResource(R.string.identity_invite_until, DateUtils.formatDateTime(context, i.expiresAt.toLong(), DateUtils.FORMAT_SHOW_DATE or DateUtils.FORMAT_SHOW_TIME))
            SettingRow(i.code, status, trailing = if (i.redeemed) null else ({
                TextButton(onClick = { client.dispatch(Command.RevokeCoordinatorInvite(CommandRevokeCoordinatorInviteInner(i.code))) }) { Text(stringResource(R.string.identity_invite_revoke)) }
            }))
        }
        OutlinedButton(onClick = { client.dispatch(Command.CreateCoordinatorInvite) }, shapes = ButtonDefaults.shapes(), modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp).testTag("identity.inviteCreate")) { Text(stringResource(R.string.identity_invite_create)) }
    }
    CoordinatorYouTubeMusic(owner = pin?.owner == true)
}

/**
 * The coordinator's YouTube Music analysis (owner only, when it runs it): signed in as / anonymous,
 * a refusal, and Sign in, which opens a fresh Google sign-in (a cleared cookie jar, cleared again
 * after) so the coordinator gets a session of its own, never this device's.
 */
@Composable
private fun ColumnScope.CoordinatorYouTubeMusic(owner: Boolean) {
    val client = LocalCoreClient.current
    val context = LocalContext.current
    val status by client.coordinatorYtm.collectAsStateWithLifecycle()
    var cancelled by remember { mutableStateOf(false) }
    val signIn = rememberLauncherForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
        val command = CoordinatorYtm.signIn(result.resultCode == Activity.RESULT_OK, result.data?.getStringExtra(YtmSignInActivity.EXTRA_COOKIES))
        cancelled = command == null
        command?.let(client::dispatch)
    }
    val row = CoordinatorYtm.row(owner, status) ?: return
    SettingsSection(stringResource(R.string.identity_coord_ytm))
    SettingRow(
        if (row.signedIn) stringResource(R.string.identity_coord_ytm_signed_in, row.account.orEmpty()) else stringResource(R.string.identity_coord_ytm_anon),
        row.error ?: if (cancelled) stringResource(R.string.identity_coord_ytm_cancelled) else null,
        tag = "identity.coordYtm",
    )
    if (row.signedIn) {
        OutlinedButton(onClick = { cancelled = false; client.dispatch(CoordinatorYtm.signOut) }, shapes = ButtonDefaults.shapes(), modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp).testTag("identity.coordYtmSignOut")) { Text(stringResource(R.string.identity_coord_ytm_sign_out)) }
    } else {
        Button(onClick = { cancelled = false; signIn.launch(YtmSignInActivity.forCoordinator(context)) }, shapes = ButtonDefaults.shapes(), modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp).testTag("identity.coordYtmSignIn")) { Text(stringResource(R.string.identity_coord_ytm_sign_in)) }
    }
}
