package app.hocket.ui.nav

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import kotlinx.coroutines.delay
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.R
import app.hocket.core.SettingKeys
import app.hocket.core.api.NetworkKind
import app.hocket.core.api.AccountInfo
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.icons.HocketIcons
import app.hocket.ui.screens.settings.setting

/** The user's (or else the server's) initial, for the avatar. */
private fun initialOf(server: AccountInfo?): String =
    (server?.username?.firstOrNull { it.isLetterOrDigit() } ?: server?.name?.firstOrNull { it.isLetterOrDigit() } ?: '?').uppercaseChar().toString()

/** A round avatar with the initial; decorative (its button or row says who). The letter is sized to the circle, not the font scale. */
@Composable
private fun Avatar(server: AccountInfo?, size: Dp) {
    val letterSize = with(LocalDensity.current) { (size * 0.5f).toSp() }
    Box(Modifier.size(size).clip(CircleShape).background(MaterialTheme.colorScheme.primaryContainer).clearAndSetSemantics { }, contentAlignment = Alignment.Center) {
        Text(initialOf(server), fontSize = letterSize, lineHeight = letterSize, fontWeight = FontWeight.Medium, color = MaterialTheme.colorScheme.onPrimaryContainer, maxLines = 1)
    }
}

/**
 * The account button of the main destinations' top app bars: a round avatar with the user's
 * initial, spoken "Account and settings". Opens [AccountSheet]. Nothing outside the main shell.
 * While the device is offline the [OfflineIndicator] stands just before it.
 */
@Composable
fun AccountButton(modifier: Modifier = Modifier) {
    val shell = LocalShellNavigator.current ?: return
    val client = LocalCoreClient.current
    val server by client.server.collectAsStateWithLifecycle()
    val description = stringResource(R.string.account_button)
    OfflineIndicator()
    IconButton(onClick = shell.openAccount, modifier = modifier.semantics { contentDescription = description }.testTag("account.button")) {
        Avatar(server, 32.dp)
    }
}

/**
 * Offline, as the core last reported it (`NetworkChanged`: the platform's `SetNetworkState`, or
 * offline whatever it says while offline mode is on): a cloud-off button, spoken "Offline. Show
 * what's available offline", that opens the Available offline list. Nothing while online or
 * outside the main shell.
 */
@Composable
fun OfflineIndicator(modifier: Modifier = Modifier) {
    val shell = LocalShellNavigator.current ?: return
    val client = LocalCoreClient.current
    val network by client.network.collectAsStateWithLifecycle()
    if (network?.kind != NetworkKind.Offline) return
    val description = stringResource(R.string.offline_indicator)
    IconButton(onClick = shell.openAvailableOffline, modifier = modifier.semantics { contentDescription = description }.testTag("offline.indicator")) {
        Icon(HocketIcons.Outlined.CloudOff, contentDescription = null)
    }
}

/**
 * Who is signed in where (and, while online, whether the server can't be reached), the offline
 * mode switch, then Settings, Listening stats, Devices (the Connect picker) and Set up
 * another device ([PairDeviceSheet]), the places not in the bottom bar (so every place stays reachable) and the bar editor.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AccountSheet(
    barItems: List<NavItem>,
    onSettings: () -> Unit,
    onStats: () -> Unit,
    onDevices: () -> Unit,
    onSetUpDevice: () -> Unit,
    onPlace: (NavItem) -> Unit,
    onEditBar: () -> Unit,
    onDismiss: () -> Unit,
) {
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true), modifier = Modifier.testTag("account.sheet")) {
        AccountSheetContent(barItems, onSettings, onStats, onDevices, onSetUpDevice, onPlace, onEditBar)
    }
}

/** The account sheet's content (also drawn on its own by the screenshot test). */
/** How long the server must stay unreachable, online, before the account sheet says so. */
private const val UNREACHABLE_HINT_DELAY_MS = 10_000L

@Composable
fun AccountSheetContent(
    barItems: List<NavItem>,
    onSettings: () -> Unit,
    onStats: () -> Unit,
    onDevices: () -> Unit,
    onSetUpDevice: () -> Unit,
    onPlace: (NavItem) -> Unit,
    onEditBar: () -> Unit,
) {
    val client = LocalCoreClient.current
    val server by client.server.collectAsStateWithLifecycle()
    val network by client.network.collectAsStateWithLifecycle()
    val offlineMode = setting(SettingKeys.NETWORK_OFFLINE_MODE)
    // Online but the server doesn't answer: worth a word, since only offline music plays then. Not
    // while offline (the top bar says so) or in offline mode (nothing is asked of it). `reachable`
    // starts false until the core's first check of the server is back, so only once it has stayed
    // false for a while online: no flash at every launch.
    val suspect = server?.reachable == false && network?.kind != NetworkKind.Offline
    var unreachable by remember { mutableStateOf(false) }
    LaunchedEffect(suspect) {
        unreachable = false
        if (suspect) {
            delay(UNREACHABLE_HINT_DELAY_MS)
            unreachable = true
        }
    }
    Column(Modifier.verticalScroll(rememberScrollState()).navigationBarsPadding().padding(bottom = 8.dp)) {
        ListItem(
            leadingContent = { Avatar(server, 48.dp) },
            headlineContent = { Text(server?.username ?: stringResource(R.string.settings_summary_no_server), style = MaterialTheme.typography.titleMedium, modifier = Modifier.semantics { heading() }) },
            supportingContent = {
                server?.let {
                    // The server's own name only when it says more than its address.
                    val text = if (it.url.contains(it.name)) it.url else stringResource(R.string.account_signed_in, it.name, it.url)
                    Column {
                        Text(text, maxLines = 2, overflow = TextOverflow.Ellipsis)
                        if (unreachable) Text(stringResource(R.string.account_server_unreachable), color = MaterialTheme.colorScheme.error, modifier = Modifier.testTag("account.unreachable"))
                    }
                }
            },
            colors = ListItemDefaults.colors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
        )
        HorizontalDivider(Modifier.padding(vertical = 4.dp))
        // Offline mode within reach: the core then reports offline, which the top bar shows.
        val offline = offlineMode.bool ?: false
        ListItem(
            leadingContent = { Icon(HocketIcons.Outlined.CloudOff, null) },
            headlineContent = { Text(stringResource(R.string.settings_offline_mode)) },
            supportingContent = { Text(stringResource(R.string.account_offline_mode_body)) },
            trailingContent = { Switch(checked = offline, onCheckedChange = null) },
            colors = ListItemDefaults.colors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
            modifier = Modifier.toggleable(value = offline, role = Role.Switch, onValueChange = { offlineMode.setBool(it) }).testTag("account.offlineMode"),
        )
        SheetRow(HocketIcons.Filled.Settings, stringResource(R.string.account_settings), "account.settings", onSettings)
        SheetRow(HocketIcons.Filled.BarChart, stringResource(R.string.account_stats), "account.stats", onStats)
        SheetRow(HocketIcons.Filled.Devices, stringResource(R.string.account_devices), "account.devices", onDevices)
        SheetRow(HocketIcons.Filled.QrCode2, stringResource(R.string.account_set_up_device), "account.setUpDevice", onSetUpDevice)
        val others = NavItem.entries.filter { it !in barItems && it != NavItem.Stats }
        if (others.isNotEmpty()) {
            HorizontalDivider(Modifier.padding(vertical = 4.dp))
            Text(stringResource(R.string.account_more_places), style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.primary,
                modifier = Modifier.padding(start = 16.dp, top = 8.dp, bottom = 4.dp).semantics { heading() })
            others.forEach { item -> SheetRow(item.icon(false), item.label(), "account.place.${item.id}") { onPlace(item) } }
        }
        HorizontalDivider(Modifier.padding(vertical = 4.dp))
        SheetRow(HocketIcons.Filled.Tune, stringResource(R.string.bottom_bar_edit), "account.editBar", onEditBar)
        Spacer(Modifier.height(8.dp))
    }
}

@Composable
private fun SheetRow(icon: ImageVector, label: String, tag: String, onClick: () -> Unit) {
    ListItem(
        leadingContent = { Icon(icon, null) },
        headlineContent = { Text(label) },
        colors = ListItemDefaults.colors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
        modifier = Modifier.clickable(onClick = onClick).testTag(tag),
    )
}

/** The bar editor as a sheet: the long-press shortcut on the bar and the account sheet's entry. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun BottomBarEditorSheet(onDismiss: () -> Unit) {
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true), modifier = Modifier.testTag("bottomBar.sheet")) {
        BottomBarEditorSheetContent()
    }
}

/** The editor sheet's content: a title, then [BottomBarEditor]. */
@Composable
fun BottomBarEditorSheetContent() {
    Column(Modifier.verticalScroll(rememberScrollState()).navigationBarsPadding().padding(bottom = 16.dp)) {
        Text(stringResource(R.string.settings_bottom_bar), style = MaterialTheme.typography.titleLarge, modifier = Modifier.padding(horizontal = 16.dp).semantics { heading() })
        BottomBarEditor()
    }
}
