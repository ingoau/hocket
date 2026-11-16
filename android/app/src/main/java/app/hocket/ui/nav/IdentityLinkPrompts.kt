package app.hocket.ui.nav

import androidx.compose.material3.AlertDialog
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.AppLinks
import app.hocket.R
import app.hocket.core.api.Command
import app.hocket.core.api.CommandClaimCoordinatorInner
import app.hocket.core.api.CommandPairWithInviteInner
import app.hocket.core.api.Event
import app.hocket.ui.LocalCoreClient

/**
 * Links that change who this device syncs with ask first: another device's pairing link (this
 * device leaves its own devices and queue for theirs) and a coordinator claim. The answers to
 * pairing and claims show as snackbars.
 */
@Composable
fun IdentityLinkPrompts(snackbar: SnackbarHostState) {
    val client = LocalCoreClient.current
    val context = LocalContext.current
    val pair by AppLinks.pair.collectAsStateWithLifecycle()
    val claim by AppLinks.claim.collectAsStateWithLifecycle()
    pair?.let { link -> PairLinkPrompt(link) { client.dispatch(Command.PairWithInvite(CommandPairWithInviteInner(link))) } }
    claim?.let { link ->
        AlertDialog(
            onDismissRequest = { AppLinks.consumeClaim(link) },
            title = { Text(stringResource(R.string.identity_claim)) },
            text = { Text("${link.first} · ${link.second}") },
            confirmButton = { TextButton(onClick = { AppLinks.consumeClaim(link); client.dispatch(Command.ClaimCoordinator(CommandClaimCoordinatorInner(link.first, link.second))) }) { Text(stringResource(R.string.identity_claim)) } },
            dismissButton = { TextButton(onClick = { AppLinks.consumeClaim(link) }) { Text(stringResource(R.string.action_cancel)) } },
        )
    }
    LaunchedEffect(client) {
        client.identityEvents.collect { e ->
            val text = when (e) {
                is Event.DevicePaired -> context.getString(R.string.pair_paired, e.data.name)
                is Event.PairingFinished -> context.getString(R.string.pair_joined)
                is Event.PairingFailed -> context.getString(R.string.pair_failed, e.data.message)
                is Event.CoordinatorJoined -> context.getString(if (e.data.owner) R.string.identity_claimed else R.string.identity_joined_coordinator)
                is Event.CoordinatorFailed -> e.data.message
                else -> null
            }
            if (text != null) snackbar.showSnackbar(text)
        }
    }
}

/**
 * Asks before pairing with [link], another device's pairing link opened from outside the app (a
 * web page can open one): pairing moves this device to the other's identity. Either answer
 * consumes the link; [onPair] runs on Pair.
 */
@Composable
fun PairLinkPrompt(link: String, onPair: () -> Unit) {
    AlertDialog(
        onDismissRequest = { AppLinks.consumePair(link) },
        title = { Text(stringResource(R.string.pair_join_title)) },
        text = { Text(stringResource(R.string.pair_join_body)) },
        confirmButton = { TextButton(onClick = { AppLinks.consumePair(link); onPair() }, modifier = Modifier.testTag("pair.prompt.confirm")) { Text(stringResource(R.string.pair_join_confirm)) } },
        dismissButton = { TextButton(onClick = { AppLinks.consumePair(link) }, modifier = Modifier.testTag("pair.prompt.cancel")) { Text(stringResource(R.string.action_cancel)) } },
        modifier = Modifier.testTag("pair.prompt"),
    )
}
