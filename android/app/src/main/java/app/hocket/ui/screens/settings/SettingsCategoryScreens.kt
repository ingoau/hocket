package app.hocket.ui.screens.settings

import app.hocket.core.api.AccountInfo
import androidx.compose.material3.IconButton
import androidx.compose.foundation.layout.Row
import androidx.compose.ui.semantics.role
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.res.pluralStringResource
import app.hocket.core.Saver
import app.hocket.core.api.SaverFeature
import app.hocket.core.api.SaverLevel
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.ToggleButton
import androidx.compose.ui.Alignment
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import app.hocket.core.api.SettingScope
import androidx.compose.material3.Icon
import androidx.compose.material3.Slider
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavHostController
import app.hocket.R
import app.hocket.ui.components.ActionRow
import app.hocket.ui.components.LabelledSlider
import androidx.compose.foundation.layout.heightIn
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import app.hocket.ui.components.ChoiceRow
import app.hocket.core.Commands
import app.hocket.core.Queries
import app.hocket.core.SettingKeys
import app.hocket.core.api.Command
import app.hocket.core.api.QueryResult
import app.hocket.playback.CoreHost
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.components.ConfirmDialog
import app.hocket.ui.components.formatAgoPhrase
import app.hocket.ui.components.formatBytes
import app.hocket.ui.icons.HocketIcons
import app.hocket.ui.nav.Route
import kotlinx.coroutines.launch
import app.hocket.AppShortcut
import app.hocket.AppShortcuts
import app.hocket.HocketApp
import kotlin.math.roundToInt

/*
 * The settings categories that are not an older sub-screen of their own (Audio, Streaming &
 * transcoding, Connect, Customise and About live in SubSettingsScreens.kt). Every row carries a
 * stable `tag` (test tag `setting.<tag>`): SettingsCategoriesTest walks every category and checks
 * that each setting is in exactly one of them.
 */

@Composable
internal fun Chevron() = Icon(HocketIcons.Filled.ChevronRight, null)

/**
 * Accounts: every account in priority order (the first plays first and holds the session), each
 * with its server, sync now / full sync, move up / down and remove; Add account; then the
 * settings-sync master switch.
 */
@Composable
fun AccountSettingsScreen(nav: NavHostController) {
    val client = LocalCoreClient.current
    val accounts by client.accounts.collectAsStateWithLifecycle()
    val sync = setting(SettingKeys.SYNC_ENABLED)
    var signOut by remember { mutableStateOf<AccountInfo?>(null) }
    fun move(from: Int, to: Int) {
        if (to !in accounts.indices) return
        val ids = accounts.map { it.id }.toMutableList()
        ids.add(to, ids.removeAt(from))
        client.dispatch(Commands.reorderAccounts(ids))
    }
    SubScreen(nav, stringResource(R.string.settings_category_account)) {
        if (accounts.size > 1) Text(stringResource(R.string.settings_accounts_order_hint), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp))
        accounts.forEachIndexed { i, s ->
            // The first account keeps the tags tests and the settings search know.
            val tag = { t: String -> if (i == 0) t else "$t.$i" }
            SettingsSection(s.name, tag = tag("server.section"))
            // The login is kept on this device only (the keystore).
            SettingRow(stringResource(R.string.settings_server_body, s.url, s.username), s.lastSync?.let { stringResource(R.string.settings_last_sync, formatAgoPhrase(it)) } ?: stringResource(R.string.settings_never_synced), scope = SettingScope.DeviceLocal, tag = tag("server.info")) {
                if (accounts.size > 1) Row {
                    IconButton(onClick = { move(i, i - 1) }, enabled = i > 0, modifier = Modifier.testTag("account.up.$i")) { Icon(HocketIcons.Filled.ArrowUpward, stringResource(R.string.settings_accounts_up, s.name)) }
                    IconButton(onClick = { move(i, i + 1) }, enabled = i < accounts.size - 1, modifier = Modifier.testTag("account.down.$i")) { Icon(HocketIcons.Filled.ArrowDownward, stringResource(R.string.settings_accounts_down, s.name)) }
                }
            }
            SettingRow(stringResource(R.string.settings_sync_now), onClick = { client.dispatch(Commands.syncLibrary(s.id, full = false)) }, tag = tag("server.syncNow"))
            SettingRow(stringResource(R.string.settings_full_sync), onClick = { client.dispatch(Commands.syncLibrary(s.id, full = true)) }, tag = tag("server.fullSync"))
            SettingRow(stringResource(R.string.settings_remove_server), onClick = { signOut = s }, tag = tag("server.remove"), destructive = true) { Icon(HocketIcons.AutoMirrored.Filled.Logout, null, tint = MaterialTheme.colorScheme.error) }
        }
        SettingRow(stringResource(R.string.settings_accounts_add), onClick = { app.hocket.ui.screens.setup.SetupRequests.addAccount() }, tag = "account.add") { Icon(HocketIcons.Filled.Add, null) }
        SwitchRow(stringResource(R.string.settings_sync_master), sync.bool ?: true, { client.dispatch(Commands.setSettingsSync(it)) }, stringResource(R.string.settings_sync_master_body), scope = sync.scope ?: SettingScope.DeviceLocal, tag = "sync.master")
    }
    // Removal goes through CoreHost.removeAccount: the stored login is removed before RemoveAccount,
    // so nothing can replay it and resurrect the account.
    signOut?.let { s -> ConfirmDialog(stringResource(R.string.settings_remove_server_confirm, s.name), stringResource(R.string.settings_remove_server), onConfirm = { CoreHost.removeAccount(client::dispatch, s) }, onDismiss = { signOut = null }) }
}

/** Appearance: theme, accent colour, colour from the artwork, animated background, immersive artwork. */
@Composable
fun AppearanceSettingsScreen(nav: NavHostController) {
    val theme = setting(SettingKeys.DISPLAY_THEME)
    val accent = setting(SettingKeys.DISPLAY_ACCENT)
    val dynamicColour = setting(SettingKeys.DISPLAY_DYNAMIC_COLOUR)
    val animated = setting(SettingKeys.DISPLAY_ANIMATED_BACKGROUND)
    SubScreen(nav, stringResource(R.string.settings_section_appearance)) {
        val themeOptions = listOf("system" to R.string.settings_theme_system, "light" to R.string.settings_theme_light, "dark" to R.string.settings_theme_dark)
            .map { (mode, label) -> VisualOption(mode, stringResource(label)) { selected -> ThemePreview(mode, selected) } }
        SettingRow(stringResource(R.string.settings_theme), scope = theme.scope, tag = "display.theme")
        OptionCards(themeOptions, isSelected = { (theme.string ?: "system") == it }, onSelect = { theme.setString(it) }, tag = "display.theme.choices", previewHeight = 72.dp)
        // display.accent: null = follow the wallpaper (dynamic) / the artwork; a #RRGGBB overrides it.
        val accentValue = accent.string
        val dynamicLabel = stringResource(R.string.settings_accent_dynamic)
        SettingRow(stringResource(R.string.settings_accent), scope = accent.scope, tag = "display.accent")
        AccentChoices(
            dynamicLabel = dynamicLabel,
            swatches = ACCENTS.map { (hex, name) -> hex to stringResource(name) },
            selected = accentValue,
            onSelect = { hex -> if (hex == null) accent.setRaw("null") else accent.setString(hex) },
            modifier = Modifier.padding(horizontal = 16.dp).padding(bottom = 8.dp),
        )
        SwitchRow(stringResource(R.string.settings_accent_artwork), dynamicColour.bool ?: true, { dynamicColour.setBool(it) }, scope = dynamicColour.scope, tag = "display.artworkColour")
        // display.immersiveArtwork: the full player's artwork edge to edge (mirrored or carried on in its colours).
        val immersive = setting(SettingKeys.DISPLAY_IMMERSIVE_ARTWORK)
        val immersiveOptions = listOf("automatic" to R.string.settings_immersive_automatic, "always" to R.string.settings_immersive_always, "never" to R.string.settings_immersive_never)
            .map { (mode, label) -> VisualOption(mode, stringResource(label)) { selected -> ImmersivePreview(mode, selected) } }
        SettingRow(stringResource(R.string.settings_immersive), scope = immersive.scope, tag = "display.immersiveArtwork")
        OptionCards(immersiveOptions, isSelected = { (immersive.string ?: "automatic") == it }, onSelect = { immersive.setString(it) }, tag = "display.immersiveArtwork.choices", previewHeight = 72.dp)
        SwitchRow(stringResource(R.string.settings_animated_background), animated.bool ?: true, { animated.setBool(it) }, scope = animated.scope, tag = "display.animatedBackground")
        val playerRating = setting(SettingKeys.DISPLAY_PLAYER_RATING)
        SwitchRow(stringResource(R.string.settings_player_rating), playerRating.bool ?: true, { playerRating.setBool(it) }, scope = playerRating.scope, tag = "display.playerRating")
        val floatingBar = setting(SettingKeys.DISPLAY_FLOATING_NAV_BAR)
        SwitchRow(stringResource(R.string.settings_floating_nav_bar), floatingBar.bool ?: true, { floatingBar.setBool(it) }, scope = floatingBar.scope, tag = "display.floatingNavBar")
        val albumArtists = setting(SettingKeys.DISPLAY_ALBUM_TRACK_ARTISTS)
        SwitchRow(stringResource(R.string.settings_album_track_artists), albumArtists.bool ?: false, { albumArtists.setBool(it) }, scope = albumArtists.scope, tag = "display.albumTrackArtists")
    }
}

/** The accent presets: `display.accent` hex and the colour's name. */
internal val ACCENTS = listOf("#6750A4" to R.string.settings_accent_purple, "#1B6B5E" to R.string.settings_accent_teal, "#B3261E" to R.string.settings_accent_red)

internal fun accentColour(hex: String): Color = Color(android.graphics.Color.parseColor(hex))

/**
 * One round swatch per choice: "Dynamic" first, drawn as a colour wheel, then each preset in its
 * real colour. The swatches are radio buttons spoken by their name ("Teal, selected"); the
 * selected one has a ring and a check.
 */
@Composable
private fun AccentChoices(dynamicLabel: String, swatches: List<Pair<String, String>>, selected: String?, onSelect: (String?) -> Unit, modifier: Modifier = Modifier) {
    val wheel = androidx.compose.ui.graphics.Brush.sweepGradient(listOf(Color(0xFFE8A0BF), Color(0xFFB8A0E8), Color(0xFF8FC1E3), Color(0xFF9ED9A8), Color(0xFFF2D58C), Color(0xFFE8A0BF)))
    FlowRow(modifier.selectableGroup(), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(4.dp), itemVerticalAlignment = Alignment.CenterVertically) {
        Swatch(dynamicLabel, selected == null, Modifier.background(wheel), tag = "accent.dynamic", icon = HocketIcons.Filled.AutoAwesome) { onSelect(null) }
        swatches.forEach { (hex, name) ->
            Swatch(name, selected.equals(hex, ignoreCase = true), Modifier.background(accentColour(hex)), tag = "accent.$hex") { onSelect(hex) }
        }
    }
}

@Composable
private fun Swatch(name: String, isSelected: Boolean, fill: Modifier, tag: String, icon: androidx.compose.ui.graphics.vector.ImageVector? = null, onClick: () -> Unit) {
    Box(
        Modifier.size(56.dp)
            .selectable(selected = isSelected, role = Role.RadioButton, onClick = onClick)
            .semantics { contentDescription = name }
            .testTag(tag),
        contentAlignment = Alignment.Center,
    ) {
        Box(
            Modifier.size(48.dp).clip(CircleShape)
                .then(if (isSelected) Modifier.border(3.dp, MaterialTheme.colorScheme.onSurface, CircleShape) else Modifier)
                .padding(if (isSelected) 6.dp else 0.dp)
                .clip(CircleShape).then(fill),
            contentAlignment = Alignment.Center,
        ) {
            if (isSelected) Icon(HocketIcons.Filled.Check, null, tint = if (icon != null) Color(0xFF1D1B20) else Color.White, modifier = Modifier.size(20.dp))
            else if (icon != null) Icon(icon, null, tint = Color(0xFF1D1B20), modifier = Modifier.size(22.dp))
        }
    }
}

/**
 * Playback & queue: the Queue section, how many recent queues to keep, autoplay, sleep-timer defaults, and
 * whether other apps (media browsers, Android Auto) may browse and control playback.
 */
@Composable
fun PlaybackSettingsScreen(nav: NavHostController) {
    val client = LocalCoreClient.current
    val queue by client.queue.collectAsStateWithLifecycle()
    val savedCap = setting(SettingKeys.QUEUE_SAVED_CAP)
    val section = setting(SettingKeys.QUEUE_SECTION)
    val externalControl = setting(SettingKeys.MEDIA_EXTERNAL_CONTROL)
    SubScreen(nav, stringResource(R.string.settings_category_playback)) {
        SwitchRow(stringResource(R.string.settings_queue_section), section.bool ?: true, { section.setBool(it) },
            subtitle = stringResource(R.string.settings_queue_section_desc), scope = section.scope, tag = "queue.section")
        val cap = savedCap.int ?: 10
        var dragging by remember(cap) { mutableFloatStateOf(cap.toFloat()) }
        val shown = dragging.roundToInt()
        val capLabel = stringResource(R.string.settings_saved_cap)
        val capValue = if (shown == 0) stringResource(R.string.settings_saved_cap_zero) else stringResource(R.string.saved_cap, shown)
        SettingRow(capLabel, capValue, savedCap.scope, tag = "queue.savedCap")
        LabelledSlider(
            value = dragging, onValueChange = { dragging = it }, onValueChangeFinished = { dragging = dragging.roundToInt().toFloat(); client.dispatch(Commands.setSavedQueueCap(dragging.roundToInt())) },
            // Continuous to the eye (49 tick dots crowded the track); snapped to whole queues on release
            // and for TalkBack's adjust, and the value is the row's subtitle above.
            valueRange = 0f..50f, steps = 49, showTicks = false, label = capLabel, valueText = capValue,
            modifier = Modifier.padding(horizontal = 16.dp).testTag("queue.savedCap.slider"),
        )
        SwitchRow(stringResource(R.string.settings_autoplay), queue.autoplay, { client.dispatch(Commands.setAutoplay(it)) }, tag = "queue.autoplay")
        val resumeAlbums = setting(SettingKeys.PLAYBACK_RESUME_ALBUMS)
        SwitchRow(stringResource(R.string.settings_resume_albums), resumeAlbums.bool ?: true, { resumeAlbums.setBool(it) }, scope = resumeAlbums.scope, tag = "playback.resumeAlbums")
        SwitchRow(stringResource(R.string.settings_external_control), externalControl.bool ?: false, { externalControl.setBool(it) },
            subtitle = stringResource(R.string.settings_external_control_summary), scope = externalControl.scope, tag = "media.externalControl")
        HomeScreenSection()
    }
}

/**
 * The home screen: pinning the now-playing widget, and which launcher shortcuts the app icon's long-press menu offers ([AppShortcuts]), at most
 * [AppShortcuts.MAX]; a device-local app preference. Hidden without the app's preferences
 * (previews, tests on a bare Application).
 */
@Composable
private fun HomeScreenSection() {
    val hocket = androidx.compose.ui.platform.LocalContext.current.applicationContext as? HocketApp ?: return
    val chosen by hocket.prefs.shortcuts.collectAsStateWithLifecycle(initialValue = null)
    val current = chosen ?: return
    val scope = androidx.compose.runtime.rememberCoroutineScope()
    val full = current.size >= AppShortcuts.MAX
    // Pinning the widget straight from here, where the launcher supports it (API 26+ launchers may not).
    val context = androidx.compose.ui.platform.LocalContext.current
    val widgets = androidx.compose.runtime.remember { android.appwidget.AppWidgetManager.getInstance(context) }
    if (widgets?.isRequestPinAppWidgetSupported == true) {
        SettingsSection(stringResource(R.string.settings_home_screen))
        SettingRow(stringResource(R.string.settings_widget_add), onClick = {
            runCatching { widgets.requestPinAppWidget(android.content.ComponentName(context, app.hocket.playback.NowPlayingWidget::class.java), null, null) }
        }, tag = "widget.add")
    }
    SettingsSection(stringResource(R.string.settings_shortcuts), tag = "home.shortcuts")
    Text(stringResource(R.string.settings_shortcuts_summary, AppShortcuts.MAX), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp))
    AppShortcut.entries.forEach { shortcut ->
        val on = shortcut in current
        SwitchRow(
            stringResource(shortcut.label), on,
            { checked ->
                if (checked && full) return@SwitchRow
                scope.launch { hocket.prefs.setShortcuts(if (checked) current + shortcut else current - shortcut) }
            },
            subtitle = if (!on && full) stringResource(R.string.settings_shortcuts_full) else null,
            tag = "shortcuts.${shortcut.id}",
        )
    }
}

/** Custom stream-cache budgets offered next to "Automatic" (decimal, as sizes are shown). */
internal val CACHE_BUDGET_CHOICES = listOf(0.5e9, 1e9, 2e9, 4e9, 8e9, 16e9)

/** "500 MB", "4 GB": round choice labels, like the warning threshold's. */
@Composable
private fun budgetChoiceLabel(bytes: Double): String =
    if (bytes < 1e9) stringResource(R.string.size_mb_whole, (bytes / 1e6).toInt()) else stringResource(R.string.size_gb_whole, (bytes / 1e9).toInt())

/**
 * Downloads & storage: the downloads screen and the storage warning; the connection (offline mode,
 * data saver, Wi-Fi only downloads, prefetch on metered connections); the stream cache (usage,
 * budget, data saved).
 */
@Composable
fun DownloadsSettingsScreen(nav: NavHostController) {
    val client = LocalCoreClient.current
    val storage by client.storage.collectAsStateWithLifecycle()
    val wifiOnly = setting(SettingKeys.DOWNLOADS_WIFI_ONLY)
    val cacheMax = setting(SettingKeys.STORAGE_CACHE_MAX_BYTES)
    val prefetchMobile = setting(SettingKeys.STORAGE_PREFETCH_ON_MOBILE_DATA)
    val offlineMode = setting(SettingKeys.NETWORK_OFFLINE_MODE)
    val dataSaver = setting(SettingKeys.NETWORK_DATA_SAVER)
    val dot = stringResource(R.string.dot_separator)
    SubScreen(nav, stringResource(R.string.settings_category_downloads)) {
        SettingRow(stringResource(R.string.nav_downloads), formatBytes(storage.downloadsBytes) + dot + stringResource(R.string.settings_cache_images, formatBytes(storage.imagesBytes)), onClick = { nav.navigate(Route.Downloads) }, tag = "open.downloads") { Chevron() }
        val warn = storage.warnThresholdBytes
        val noLimit = stringResource(R.string.downloads_threshold_none)
        ChoiceDialogRow(
            stringResource(R.string.downloads_threshold),
            listOf(null, 2.0, 4.0, 8.0, 16.0).map { gb -> DialogOption(gb, gb?.let { stringResource(R.string.size_gb_whole, it.toInt()) } ?: noLimit) },
            selected = warn?.let { it / 1e9 }, onSelect = { gb -> client.dispatch(Commands.setStorageWarnThreshold(gb?.let { it * 1e9 })) },
            tag = "storage.warnThreshold", valueText = warn?.let { formatBytes(it) } ?: noLimit,
        )

        // Metered = mobile data, a hotspot or metered Wi-Fi, as the system reports it.
        SettingsSection(stringResource(R.string.settings_section_connection))
        SwitchRow(stringResource(R.string.settings_offline_mode), offlineMode.bool ?: false, { offlineMode.setBool(it) }, stringResource(R.string.settings_offline_mode_body), offlineMode.scope, tag = "network.offlineMode")
        SwitchRow(stringResource(R.string.settings_data_saver), dataSaver.bool ?: true, { dataSaver.setBool(it) }, scope = dataSaver.scope, tag = "network.dataSaver")
        SwitchRow(stringResource(R.string.settings_downloads_wifi_only), wifiOnly.bool ?: true, { wifiOnly.setBool(it) }, scope = wifiOnly.scope, tag = "downloads.wifiOnly")
        SwitchRow(stringResource(R.string.settings_prefetch_mobile), prefetchMobile.bool ?: false, { prefetchMobile.setBool(it) }, scope = prefetchMobile.scope, tag = "storage.prefetchOnMobileData")

        SettingsSection(stringResource(R.string.settings_cache_usage))
        // Usage: complete entries play offline; partial ones (seeks, skips, primed starts) do not.
        val partial = storage.partialCacheBytes ?: 0.0
        val complete = (storage.cacheBytes - partial).coerceAtLeast(0.0)
        val budget = storage.cacheBudgetBytes ?: 0.0
        SettingRow(stringResource(R.string.settings_cache_in_use), stringResource(R.string.settings_cache_usage_body, formatBytes(complete), formatBytes(partial), formatBytes(budget)), tag = "storage.cacheUsage")
        if (budget > 0) {
            LinearProgressIndicator(progress = { (storage.cacheBytes / budget).toFloat().coerceIn(0f, 1f) }, modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp).padding(bottom = 8.dp).clearAndSetSemantics { })
        }
        // Budget: "Automatic" while storage.cacheMaxBytes is null (its default); any size, 2 GB
        // included, is the user's own.
        val auto = storage.cacheBudgetAuto ?: (cacheMax.double == null)
        val autoLabel = stringResource(R.string.settings_cache_budget_auto, formatBytes(budget))
        // The dialog's pick: Automatic (null), or the size the budget is set to (snapped to the nearest choice).
        val picked = if (auto) null else CACHE_BUDGET_CHOICES.firstOrNull { kotlin.math.abs(budget - it) < 1e6 } ?: budget
        ChoiceDialogRow(
            stringResource(R.string.settings_cache_budget),
            listOf(DialogOption<Double?>(null, stringResource(R.string.settings_cache_budget_auto_short))) + CACHE_BUDGET_CHOICES.map { DialogOption<Double?>(it, budgetChoiceLabel(it)) },
            selected = picked, onSelect = { b -> if (b == null) cacheMax.setNull() else cacheMax.setDouble(b) },
            scope = cacheMax.scope, tag = "storage.cacheMaxBytes",
            valueText = if (auto) autoLabel else stringResource(R.string.settings_cache_budget_custom, formatBytes(budget)),
        )
        SettingRow(stringResource(R.string.settings_data_saved), stringResource(R.string.settings_data_saved_body, formatBytes(storage.servedFromDiskBytes ?: 0.0), formatBytes(storage.fetchedBytes ?: 0.0)), tag = "storage.dataSaved") {
            Text(formatBytes(storage.dataSavedBytes ?: 0.0), style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.primary)
        }
        var clearing by remember { mutableStateOf(false) }
        SettingRow(stringResource(R.string.downloads_clear_cache), formatBytes(storage.cacheBytes), onClick = { clearing = true }, tag = "storage.clearCache")
        if (clearing) app.hocket.ui.components.ConfirmDialog(stringResource(R.string.downloads_clear_cache_confirm, formatBytes(storage.cacheBytes)), stringResource(R.string.downloads_clear_cache_action),
            onConfirm = { client.dispatch(Command.ClearStreamCache) }, onDismiss = { clearing = false })
    }
}

/** Lyrics: external lookups, the default timing offset, translations. */
@Composable
fun LyricsSettingsScreen(nav: NavHostController) {
    val client = LocalCoreClient.current
    val externalLyrics = setting(SettingKeys.LYRICS_EXTERNAL_ENABLED)
    val offset = setting(SettingKeys.LYRICS_DEFAULT_OFFSET_MS)
    SubScreen(nav, stringResource(R.string.settings_section_lyrics)) {
        SwitchRow(stringResource(R.string.settings_external_lyrics), externalLyrics.bool ?: false, { client.dispatch(Commands.setExternalLyricsEnabled(it)) }, stringResource(R.string.settings_external_lyrics_privacy), externalLyrics.scope, tag = "lyrics.external")
        val ms = offset.int ?: 0
        StepperRow(
            stringResource(R.string.settings_lyrics_default_offset), stringResource(R.string.lyrics_offset_value, ms),
            onLess = { offset.setInt((ms - 100).coerceAtLeast(-10_000)) }, onMore = { offset.setInt((ms + 100).coerceAtMost(10_000)) },
            lessLabel = stringResource(R.string.lyrics_offset_earlier_a11y), moreLabel = stringResource(R.string.lyrics_offset_later_a11y),
            scope = offset.scope, tag = "lyrics.defaultOffsetMs",
            onReset = { offset.setInt(0) }.takeIf { ms != 0 }, resetLabel = stringResource(R.string.lyrics_offset_reset_a11y),
        )
    }
}

/**
 * Library & ratings: the love bridge, smart filters, listening stats; then library sync (how often
 * to check the server for changes, how often to run the full check that catches deletions) and
 * whether search asks the server too.
 */
@Composable
fun LibrarySettingsScreen(nav: NavHostController) {
    val loveBridge = setting(SettingKeys.RATINGS_LOVE_BRIDGE_ENABLED)
    val loveThreshold = setting(SettingKeys.RATINGS_LOVE_BRIDGE_THRESHOLD)
    val syncInterval = setting(SettingKeys.LIBRARY_SYNC_INTERVAL_MINUTES)
    val reconcileDays = setting(SettingKeys.LIBRARY_FULL_RECONCILE_DAYS)
    val searchServer = setting(SettingKeys.SEARCH_INCLUDE_SERVER)
    val resources = androidx.compose.ui.platform.LocalResources.current
    SubScreen(nav, stringResource(R.string.settings_category_library)) {
        // ratings.loveBridge.enabled + threshold (1..5): "off" is the bridge disabled.
        val bridgeOn = loveBridge.bool ?: false
        val threshold = loveThreshold.int ?: 4
        SettingRow(stringResource(R.string.settings_love_threshold), if (!bridgeOn) stringResource(R.string.settings_love_threshold_off) else stringResource(R.string.settings_love_threshold_value, threshold), loveThreshold.scope, tag = "ratings.loveThreshold")
        LoveThresholdPicker(if (bridgeOn) threshold else null,
            onSelect = { n -> if (n == null) loveBridge.setBool(false) else { loveThreshold.setInt(n); loveBridge.setBool(true) } })
        SettingRow(stringResource(R.string.nav_filters), onClick = { nav.navigate(Route.Filters) }, tag = "open.filters") { Chevron() }
        SettingRow(stringResource(R.string.nav_stats), onClick = { nav.navigate(Route.Stats) }, tag = "open.stats") { Chevron() }

        SettingsSection(stringResource(R.string.settings_section_library_sync))
        // library.syncIntervalMinutes (5..1440, default 60) and library.fullReconcileDays (1..90, default 7).
        val minutes = syncInterval.int ?: 60
        ChoiceDialogRow(
            stringResource(R.string.settings_sync_interval),
            SYNC_INTERVAL_CHOICES.map { m -> DialogOption(m, if (m < 60) resources.getQuantityString(R.plurals.a11y_minutes, m, m) else resources.getQuantityString(R.plurals.a11y_hours, m / 60, m / 60)) },
            selected = minutes, onSelect = { syncInterval.setInt(it) }, scope = syncInterval.scope, tag = "library.syncIntervalMinutes",
            valueText = stringResource(R.string.settings_sync_interval_value, intervalLabel(minutes)),
        )
        val days = reconcileDays.int ?: 7
        ChoiceDialogRow(
            stringResource(R.string.settings_full_reconcile),
            RECONCILE_DAY_CHOICES.map { d -> DialogOption(d, resources.getQuantityString(R.plurals.a11y_days, d, d)) },
            selected = days, onSelect = { reconcileDays.setInt(it) }, scope = reconcileDays.scope, tag = "library.fullReconcileDays",
            valueText = pluralStringResource(R.plurals.settings_full_reconcile_value, days, days),
        )
        SwitchRow(stringResource(R.string.settings_search_include_server), searchServer.bool ?: true, { searchServer.setBool(it) }, scope = searchServer.scope, tag = "search.includeServer")
    }
}

/**
 * The love threshold as stars: tapping the third star loves songs rated three or more, so the
 * stars up to it fill; "Off" turns the bridge off. Each is one radio button ("3 stars, selected").
 */
@Composable
private fun LoveThresholdPicker(threshold: Int?, onSelect: (Int?) -> Unit) {
    val resources = androidx.compose.ui.platform.LocalResources.current
    val scheme = MaterialTheme.colorScheme
    // Off and the stars wrap as two pieces (at a large font, the stars move under Off rather than squeeze).
    FlowRow(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp).padding(bottom = 8.dp).selectableGroup().testTag("ratings.loveThreshold.stars"),
        horizontalArrangement = Arrangement.spacedBy(8.dp), itemVerticalAlignment = Alignment.CenterVertically,
    ) {
        androidx.compose.material3.FilterChip(
            selected = threshold == null, onClick = { onSelect(null) },
            label = { Text(stringResource(R.string.settings_love_threshold_off)) },
            modifier = Modifier.semantics { role = Role.RadioButton }.testTag("ratings.loveThreshold.off"),
        )
        Row { (1..5).forEach { n ->
            val filled = threshold != null && n <= threshold
            Box(
                Modifier.size(48.dp).clip(CircleShape)
                    .selectable(selected = threshold == n, role = Role.RadioButton, onClick = { onSelect(n) })
                    .semantics { contentDescription = resources.getQuantityString(R.plurals.a11y_stars, n, n) }
                    .testTag("ratings.loveThreshold.$n"),
                contentAlignment = Alignment.Center,
            ) {
                Icon(if (filled) HocketIcons.Filled.Star else HocketIcons.Filled.StarOutline, null,
                    tint = if (filled) scheme.primary else scheme.onSurfaceVariant, modifier = Modifier.size(32.dp))
            }
        } }
    }
}

private val SYNC_INTERVAL_CHOICES = listOf(15, 30, 60, 180, 360, 1440)
private val RECONCILE_DAY_CHOICES = listOf(1, 3, 7, 14, 30)

/** "15 min", "1 h", "24 h". */
@Composable
private fun intervalLabel(minutes: Int): String =
    if (minutes < 60 || minutes % 60 != 0) stringResource(R.string.sleep_minutes, minutes) else stringResource(R.string.duration_hours, minutes / 60)

/**
 * Battery: the saver level (a four-stop slider, Off to Max), the switches it stands for (editing
 * one makes the level Custom, starting from what the chosen level turns off), and whether the
 * system's battery saver engages it, at which level.
 */
@Composable
fun BatterySettingsScreen(nav: NavHostController) {
    val client = LocalCoreClient.current
    val saver by client.batterySaver.collectAsStateWithLifecycle()
    val levelSetting = setting(SettingKeys.BATTERY_LEVEL)
    val customSetting = setting(SettingKeys.BATTERY_CUSTOM)
    val autoSaver = setting(SettingKeys.BATTERY_AUTO_ENGAGE)
    val autoLevelSetting = setting(SettingKeys.BATTERY_AUTO_LEVEL)
    val storedLevel = Saver.parseLevel(levelSetting.raw) ?: SaverLevel.Off
    val storedCustom = Saver.parseCustom(customSetting.raw)
    // What this screen last wrote, shown until the core reports it back: the slider does not snap
    // back to the old stop on release, and quick toggles build on each other instead of on the
    // last reported set. Dropped once the setting matches (or after a while, if it never does).
    var pendingLevel by remember { mutableStateOf<SaverLevel?>(null) }
    var pendingCustom by remember { mutableStateOf<List<SaverFeature>?>(null) }
    LaunchedEffect(pendingLevel, storedLevel) {
        val p = pendingLevel ?: return@LaunchedEffect
        if (p == storedLevel) pendingLevel = null else { kotlinx.coroutines.delay(PENDING_SETTING_MS); pendingLevel = null }
    }
    LaunchedEffect(pendingCustom, customSetting.raw) {
        val p = pendingCustom ?: return@LaunchedEffect
        if (Saver.customValue(p) == Saver.customValue(storedCustom)) pendingCustom = null else { kotlinx.coroutines.delay(PENDING_SETTING_MS); pendingCustom = null }
    }
    val chosen = pendingLevel ?: storedLevel
    val chosenFeatures = Saver.features(chosen, pendingCustom ?: storedCustom)
    SubScreen(nav, stringResource(R.string.settings_section_battery)) {
        val title = stringResource(R.string.settings_battery_saver)
        val levelName = saverLevelName(chosen)
        val summary = saverLevelSummary(chosen, chosenFeatures.size)
        SettingRow(title, stringResource(R.string.settings_battery_level_value, levelName, summary), levelSetting.scope, tag = "battery.level")
        // A preset sits on its stop. Custom has no stop: the track loses its ticks and the thumb
        // sits between stops, as far along as the share of savings it turns on. A drag is held
        // locally and committed on release (the tick count changing mid-drag would restart the slider).
        val presetIndex = Saver.PRESETS.indexOf(chosen)
        var drag by remember { mutableStateOf<Float?>(null) }
        val last = Saver.PRESETS.lastIndex.toFloat()
        val value = drag ?: if (presetIndex >= 0) presetIndex.toFloat() else last * chosenFeatures.size / Saver.ALL.size
        val shownName = drag?.let { saverLevelName(Saver.PRESETS[it.roundToInt()]) } ?: levelName
        LabelledSlider(
            value = value, onValueChange = { drag = it },
            onValueChangeFinished = {
                drag?.let {
                    val level = Saver.PRESETS[it.roundToInt()]
                    pendingLevel = level
                    client.dispatch(Commands.setSetting(SettingKeys.BATTERY_LEVEL, Saver.levelValue(level)))
                }
                drag = null
            },
            valueRange = 0f..last, steps = Saver.PRESETS.size - 2, showTicks = presetIndex >= 0,
            label = title, valueText = shownName,
            modifier = Modifier.padding(horizontal = 16.dp).testTag("battery.level.slider"),
        )
        if (saver.automatic) {
            Text(stringResource(R.string.settings_battery_automatic_note, saverLevelName(saver.level)), style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp).testTag("battery.automaticNote"))
        }

        var expanded by rememberSaveable { mutableStateOf(chosen == SaverLevel.Custom) }
        val turn by animateFloatAsState(if (expanded) 180f else 0f, label = "customiseArrow")
        SettingRow(stringResource(R.string.settings_battery_customise), onClick = { expanded = !expanded }, tag = "battery.customise") {
            Icon(HocketIcons.Filled.KeyboardArrowDown, null, Modifier.graphicsLayer { rotationZ = turn })
        }
        AnimatedVisibility(expanded) {
            androidx.compose.foundation.layout.Column {
                Saver.ALL.forEach { feature ->
                    val (name, body) = saverFeatureText(feature)
                    SwitchRow(name, feature in chosenFeatures, { on ->
                        val next = (if (on) chosenFeatures + feature else chosenFeatures - feature).distinct().sorted()
                        pendingCustom = next
                        client.dispatch(Commands.setSetting(SettingKeys.BATTERY_CUSTOM, Saver.customValue(next)))
                        if (chosen != SaverLevel.Custom) {
                            pendingLevel = SaverLevel.Custom
                            client.dispatch(Commands.setSetting(SettingKeys.BATTERY_LEVEL, Saver.levelValue(SaverLevel.Custom)))
                        }
                    }, subtitle = body, tag = "battery.feature.${feature.string}")
                }
            }
        }

        val autoOn = autoSaver.bool ?: true
        SwitchRow(stringResource(R.string.settings_battery_auto), autoOn, { autoSaver.setBool(it) }, scope = autoSaver.scope, tag = "battery.autoEngage")
        if (autoOn) {
            val autoLevel = Saver.parseLevel(autoLevelSetting.raw)?.takeIf { it != SaverLevel.Off } ?: SaverLevel.Medium
            SettingRow(stringResource(R.string.settings_battery_auto_level), scope = autoLevelSetting.scope, tag = "battery.autoLevel")
            val options = listOf(SaverLevel.Light to 1, SaverLevel.Medium to 2, SaverLevel.Max to 3, SaverLevel.Custom to 0)
                .map { (level, leaves) -> VisualOption(level, saverLevelName(level)) { selected -> SaverPreview(leaves, selected) } }
            OptionCards(options, isSelected = { it == autoLevel }, onSelect = { autoLevelSetting.setRaw(Saver.levelValue(it)) }, tag = "battery.autoLevel.choices", previewHeight = 48.dp)
        }
    }
}

/** How long the battery screen shows a level or custom set it wrote before trusting the stored one again. */
private const val PENDING_SETTING_MS = 3_000L

@Composable
internal fun saverLevelName(level: SaverLevel): String = stringResource(
    when (level) {
        SaverLevel.Off -> R.string.saver_level_off
        SaverLevel.Light -> R.string.saver_level_light
        SaverLevel.Medium -> R.string.saver_level_medium
        SaverLevel.Max -> R.string.saver_level_max
        SaverLevel.Custom -> R.string.saver_level_custom
    },
)

@Composable
private fun saverLevelSummary(level: SaverLevel, customCount: Int): String = when (level) {
    SaverLevel.Off -> stringResource(R.string.saver_summary_off)
    SaverLevel.Light -> stringResource(R.string.saver_summary_light)
    SaverLevel.Medium -> stringResource(R.string.saver_summary_medium)
    SaverLevel.Max -> stringResource(R.string.saver_summary_max)
    SaverLevel.Custom -> pluralStringResource(R.plurals.saver_summary_custom, customCount, customCount, Saver.ALL.size)
}

@Composable
private fun saverFeatureText(feature: SaverFeature): Pair<String, String> {
    val (name, body) = when (feature) {
        SaverFeature.StillBackground -> R.string.saver_feature_still_background to R.string.saver_feature_still_background_body
        SaverFeature.StillSeekWave -> R.string.saver_feature_still_seek_wave to R.string.saver_feature_still_seek_wave_body
        SaverFeature.LighterLyrics -> R.string.saver_feature_lighter_lyrics to R.string.saver_feature_lighter_lyrics_body
        SaverFeature.RefreshCap -> R.string.saver_feature_refresh_cap to R.string.saver_feature_refresh_cap_body
        SaverFeature.SlowSync -> R.string.saver_feature_slow_sync to R.string.saver_feature_slow_sync_body
        SaverFeature.SimpleLyrics -> R.string.saver_feature_simple_lyrics to R.string.saver_feature_simple_lyrics_body
        SaverFeature.NoImmersiveArtwork -> R.string.saver_feature_no_immersive_artwork to R.string.saver_feature_no_immersive_artwork_body
        SaverFeature.NoArtworkBlur -> R.string.saver_feature_no_artwork_blur to R.string.saver_feature_no_artwork_blur_body
        SaverFeature.SmallArtwork -> R.string.saver_feature_small_artwork to R.string.saver_feature_small_artwork_body
    }
    return stringResource(name) to stringResource(body)
}

/** Backup & diagnostics: export (optionally with secrets), import, copy diagnostics. */
@Composable
fun BackupSettingsScreen(nav: NavHostController) {
    val client = LocalCoreClient.current
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var importConfirm by remember { mutableStateOf<String?>(null) }
    var includeSecrets by remember { mutableStateOf(false) }
    val exportLauncher = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/json")) { uri ->
        uri ?: return@rememberLauncherForActivityResult
        scope.launch {
            val doc = (client.query(Queries.configDocument(includeSecrets)) as? QueryResult.Text)?.data ?: return@launch
            context.contentResolver.openOutputStream(uri)?.use { it.write(doc.toByteArray()) }
        }
    }
    val importLauncher = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        uri ?: return@rememberLauncherForActivityResult
        val text = context.contentResolver.openInputStream(uri)?.use { it.readBytes().decodeToString() } ?: return@rememberLauncherForActivityResult
        importConfirm = text
    }
    SubScreen(nav, stringResource(R.string.settings_category_backup)) {
        SwitchRow(stringResource(R.string.settings_export_with_secrets), includeSecrets, { includeSecrets = it }, tag = "backup.includeSecrets")
        SettingRow(stringResource(R.string.settings_export_config), onClick = { exportLauncher.launch("hocket-config.json") }, tag = "backup.export")
        SettingRow(stringResource(R.string.settings_import_config), onClick = { importLauncher.launch(arrayOf("application/json", "text/plain")) }, tag = "backup.import")
        SettingRow(stringResource(R.string.settings_copy_diagnostics), onClick = {
            scope.launch {
                val text = (client.query(app.hocket.core.api.Query.Diagnostics) as? QueryResult.Text)?.data ?: ""
                (context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager).setPrimaryClip(ClipData.newPlainText("Hocket diagnostics", text))
            }
        }, tag = "backup.diagnostics")
    }
    importConfirm?.let { doc -> ConfirmDialog(stringResource(R.string.settings_import_confirm), stringResource(R.string.settings_import_config), onConfirm = { client.dispatch(Commands.importConfig(doc)) }, onDismiss = { importConfirm = null }, destructive = false) }
}
