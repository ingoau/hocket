package app.hocket.ui.screens.settings

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.LargeFlexibleTopAppBar
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavHostController
import app.hocket.R
import app.hocket.core.SettingKeys
import app.hocket.core.api.ConnectionTier
import app.hocket.core.api.ReplayGainMode
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.components.formatAgo
import app.hocket.ui.components.formatBytes
import app.hocket.ui.icons.HocketIcons
import app.hocket.ui.nav.BottomContentInset
import app.hocket.ui.nav.Route

/** The settings categories, in the order they are listed. [id] is the stable test id. */
enum class SettingsCategory(val id: String, val route: Route) {
    Account("account", Route.SettingsAccount),
    Appearance("appearance", Route.SettingsAppearance),
    Playback("playback", Route.SettingsPlayback),
    Audio("audio", Route.AudioSettings),
    Streaming("streaming", Route.TranscodingSettings),
    Downloads("downloads", Route.SettingsDownloads),
    Lyrics("lyrics", Route.SettingsLyrics),
    Library("library", Route.SettingsLibrary),
    Battery("battery", Route.SettingsBattery),
    Connect("connect", Route.ConnectSettings),
    Customise("customise", Route.CustomiseSettings),
    Backup("backup", Route.SettingsBackup),
    About("about", Route.About);

    companion object {
        /** How the list is grouped (Android system-settings style: related categories share a card). */
        val GROUPS: List<List<SettingsCategory>> = listOf(
            listOf(Account),
            listOf(Appearance, Playback, Audio, Streaming),
            listOf(Downloads, Lyrics, Library, Battery),
            listOf(Connect, Customise),
            listOf(Backup, About),
        )
    }
}

/**
 * Top-level settings: one row per category (leading icon, title, a one-line summary of the current
 * values, chevron), grouped in cards; each opens its own sub-screen. Every setting lives in exactly
 * one category.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(nav: NavHostController) {
    val scroll = TopAppBarDefaults.exitUntilCollapsedScrollBehavior()
    Scaffold(
        modifier = Modifier.nestedScroll(scroll.nestedScrollConnection).testTag("settings.categories"),
        topBar = { LargeFlexibleTopAppBar(title = { Text(stringResource(R.string.settings_title), modifier = Modifier.semantics { heading() }) }, scrollBehavior = scroll) },
    ) { padding ->
        Column(
            Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(top = padding.calculateTopPadding(), bottom = BottomContentInset).padding(horizontal = 16.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            SettingsCategory.GROUPS.forEach { group ->
                Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    group.forEachIndexed { i, category ->
                        CategoryRow(category, first = i == 0, last = i == group.lastIndex, onClick = { nav.navigate(category.route) })
                    }
                }
            }
        }
    }
}

@Composable
private fun CategoryRow(category: SettingsCategory, first: Boolean, last: Boolean, onClick: () -> Unit) {
    val outer = 24.dp
    val inner = 6.dp
    val shape = RoundedCornerShape(topStart = if (first) outer else inner, topEnd = if (first) outer else inner, bottomStart = if (last) outer else inner, bottomEnd = if (last) outer else inner)
    Surface(shape = shape, color = MaterialTheme.colorScheme.surfaceContainer, modifier = Modifier.fillMaxWidth()) {
        ListItem(
            headlineContent = { Text(category.title()) },
            supportingContent = { Text(category.summary(), maxLines = 1, overflow = TextOverflow.Ellipsis) },
            leadingContent = { Icon(category.icon(), null, tint = MaterialTheme.colorScheme.primary) },
            trailingContent = { Icon(HocketIcons.Filled.ChevronRight, null) },
            colors = ListItemDefaults.colors(containerColor = MaterialTheme.colorScheme.surfaceContainer),
            modifier = Modifier.clickable(onClick = onClick).testTag("settings.category.${category.id}"),
        )
    }
}

private fun SettingsCategory.icon(): ImageVector = when (this) {
    SettingsCategory.Account -> HocketIcons.Filled.AccountCircle
    SettingsCategory.Appearance -> HocketIcons.Filled.Palette
    SettingsCategory.Playback -> HocketIcons.AutoMirrored.Filled.QueueMusic
    SettingsCategory.Audio -> HocketIcons.Filled.GraphicEq
    SettingsCategory.Streaming -> HocketIcons.Filled.NetworkCheck
    SettingsCategory.Downloads -> HocketIcons.Filled.Download
    SettingsCategory.Lyrics -> HocketIcons.Filled.Lyrics
    SettingsCategory.Library -> HocketIcons.Filled.LibraryMusic
    SettingsCategory.Battery -> HocketIcons.Filled.BatterySaver
    SettingsCategory.Connect -> HocketIcons.Filled.Devices
    SettingsCategory.Customise -> HocketIcons.Filled.Tune
    SettingsCategory.Backup -> HocketIcons.Filled.SettingsBackupRestore
    SettingsCategory.About -> HocketIcons.Filled.Info
}

@Composable
fun SettingsCategory.title(): String = stringResource(
    when (this) {
        SettingsCategory.Account -> R.string.settings_category_account
        SettingsCategory.Appearance -> R.string.settings_section_appearance
        SettingsCategory.Playback -> R.string.settings_category_playback
        SettingsCategory.Audio -> R.string.settings_section_audio
        SettingsCategory.Streaming -> R.string.settings_category_streaming
        SettingsCategory.Downloads -> R.string.settings_category_downloads
        SettingsCategory.Lyrics -> R.string.settings_section_lyrics
        SettingsCategory.Library -> R.string.settings_category_library
        SettingsCategory.Battery -> R.string.settings_section_battery
        SettingsCategory.Connect -> R.string.settings_category_connect
        SettingsCategory.Customise -> R.string.settings_section_customise
        SettingsCategory.Backup -> R.string.settings_category_backup
        SettingsCategory.About -> R.string.settings_section_about
    },
)

/** One line of the category's current values. */
@Composable
private fun SettingsCategory.summary(): String {
    val client = LocalCoreClient.current
    val dot = stringResource(R.string.dot_separator)
    val onLabel = stringResource(R.string.settings_on)
    val offLabel = stringResource(R.string.settings_off)
    fun on(b: Boolean) = if (b) onLabel else offLabel
    return when (this) {
        SettingsCategory.Account -> {
            val server by client.server.collectAsStateWithLifecycle()
            val s = server
            if (s == null) stringResource(R.string.settings_summary_no_server)
            else stringResource(R.string.settings_server_body, s.url, s.username) + dot + (s.lastSync?.let { stringResource(R.string.settings_last_sync, formatAgo(it)) } ?: stringResource(R.string.settings_never_synced))
        }
        SettingsCategory.Appearance -> {
            val theme = setting(SettingKeys.DISPLAY_THEME).string ?: "system"
            val accent = setting(SettingKeys.DISPLAY_ACCENT).string
            val themeLabel = stringResource(when (theme) { "light" -> R.string.settings_theme_light; "dark" -> R.string.settings_theme_dark; else -> R.string.settings_theme_system })
            val accentName = accent?.let { hex -> ACCENTS.firstOrNull { it.first.equals(hex, true) }?.let { stringResource(it.second) } ?: hex }
            stringResource(R.string.settings_theme) + ": " + themeLabel + dot + stringResource(R.string.settings_accent) + ": " + (accentName ?: stringResource(R.string.settings_accent_dynamic))
        }
        SettingsCategory.Playback -> {
            val queue by client.queue.collectAsStateWithLifecycle()
            val cap = setting(SettingKeys.QUEUE_SAVED_CAP).int ?: 10
            stringResource(R.string.settings_autoplay_short) + ": " + on(queue.autoplay) + dot + stringResource(R.string.settings_summary_saved_cap, cap)
        }
        SettingsCategory.Audio -> {
            val audio by client.audio.collectAsStateWithLifecycle()
            val rg = stringResource(when (audio.replayGain) { ReplayGainMode.Off -> R.string.settings_rg_off; ReplayGainMode.Track -> R.string.settings_rg_track; ReplayGainMode.Album -> R.string.settings_rg_album; ReplayGainMode.Auto -> R.string.settings_rg_auto })
            stringResource(R.string.settings_replay_gain) + ": " + rg + dot + stringResource(R.string.settings_gapless) + ": " + on(audio.gapless)
        }
        SettingsCategory.Streaming -> stringResource(R.string.settings_summary_streaming)
        SettingsCategory.Downloads -> {
            val storage by client.storage.collectAsStateWithLifecycle()
            stringResource(R.string.settings_summary_downloads, formatBytes(storage.downloadsBytes), formatBytes(storage.cacheBytes))
        }
        SettingsCategory.Lyrics -> stringResource(R.string.settings_summary_external_lyrics) + ": " + on(setting(SettingKeys.LYRICS_EXTERNAL_ENABLED).bool ?: false) +
            dot + stringResource(R.string.lyrics_offset_value, setting(SettingKeys.LYRICS_DEFAULT_OFFSET_MS).int ?: 0)
        SettingsCategory.Library -> {
            val bridge = setting(SettingKeys.RATINGS_LOVE_BRIDGE_ENABLED).bool ?: false
            val threshold = setting(SettingKeys.RATINGS_LOVE_BRIDGE_THRESHOLD).int ?: 4
            stringResource(R.string.settings_love_threshold) + ": " + (if (bridge) "$threshold★" else stringResource(R.string.settings_love_threshold_off)) + dot + stringResource(R.string.nav_filters) + dot + stringResource(R.string.nav_stats)
        }
        SettingsCategory.Battery -> {
            val saver by client.batterySaver.collectAsStateWithLifecycle()
            stringResource(R.string.settings_battery_saver) + ": " + on(saver)
        }
        SettingsCategory.Connect -> {
            val connection by client.connection.collectAsStateWithLifecycle()
            when (connection.tier) {
                ConnectionTier.Local -> stringResource(R.string.connection_local)
                ConnectionTier.Lan -> stringResource(R.string.connection_lan, connection.peerCount.toInt())
                ConnectionTier.Coordinator -> stringResource(R.string.connection_coordinator, connection.peerCount.toInt())
            }
        }
        SettingsCategory.Customise -> stringResource(R.string.settings_bottom_bar) + dot + stringResource(R.string.settings_context_menu) + dot + stringResource(R.string.settings_media_buttons) + dot + stringResource(R.string.settings_swipe_actions)
        SettingsCategory.Backup -> stringResource(R.string.settings_summary_backup)
        SettingsCategory.About -> stringResource(R.string.settings_licence)
    }
}

@Composable
internal fun str(id: Int): String = stringResource(id)
