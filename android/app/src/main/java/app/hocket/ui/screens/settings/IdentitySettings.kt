package app.hocket.ui.screens.settings

import android.text.format.DateUtils
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
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.R
import app.hocket.core.api.Command
import app.hocket.core.api.CommandClaimCoordinatorInner
import app.hocket.core.api.CommandRedeemCoordinatorInviteInner
import app.hocket.core.api.CommandRemoveDeviceInner
import app.hocket.core.api.CommandRevokeCoordinatorInviteInner
import app.hocket.core.api.CommandSetCredentialSyncInner
import app.hocket.core.api.IdentityDevice
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.nav.PairDeviceSheet
import app.hocket.ui.screens.setup.PairingLinks

/**
 * Settings → Connect's devices: the identity's devices (when each last synced, and Remove, which
 * says to change passwords), Add a device, credential sync (saying the coordinator holds the key
 * while it's on), and the coordinator: claim it with a `hocket-coordinator pair` code (typed or
 * scanned), join one with an invite, and the owner's invites.
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
    var url by rememberSaveable { mutableStateOf("") }
    var code by rememberSaveable { mutableStateOf("") }
    val pin = identity.coordinator
    LaunchedEffect(pin?.owner, pin?.url) { if (pin?.owner == true) client.dispatch(Command.ListCoordinatorInvites) }
    SettingsSection(stringResource(R.string.identity_coordinator))
    SettingRow(
        when {
            pin == null -> stringResource(R.string.identity_coordinator_none)
            pin.owner -> stringResource(R.string.identity_coordinator_owner, pin.url)
            else -> stringResource(R.string.identity_coordinator_guest, pin.url)
        }
    )
    OutlinedTextField(value = url, onValueChange = { url = it }, label = { Text(stringResource(R.string.identity_coordinator_url)) }, singleLine = true, modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp).testTag("identity.coordinatorUrl"))
    OutlinedTextField(value = code, onValueChange = { code = it }, label = { Text(stringResource(R.string.identity_coordinator_code)) }, singleLine = true, modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp).testTag("identity.coordinatorCode"))
    Row(Modifier.padding(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Button(shapes = ButtonDefaults.shapes(), enabled = code.isNotBlank(), onClick = { client.dispatch(Command.ClaimCoordinator(CommandClaimCoordinatorInner(url.trim(), code.trim()))) }, modifier = Modifier.testTag("identity.claim")) { Text(stringResource(R.string.identity_claim)) }
        TextButton(enabled = code.isNotBlank() && url.isNotBlank(), onClick = { client.dispatch(Command.RedeemCoordinatorInvite(CommandRedeemCoordinatorInviteInner(url.trim(), code.trim()))) }) { Text(stringResource(R.string.identity_redeem)) }
        if (app.hocket.SetupScanner.available) {
            TextButton(onClick = {
                app.hocket.SetupScanner.scan(context, onResult = { text -> PairingLinks.claim(text)?.let { (u, c) -> if (u.isNotEmpty()) url = u; code = c } ?: run { code = text.trim() } }, onError = {})
            }) { Text(stringResource(R.string.identity_scan_claim)) }
        }
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
}
