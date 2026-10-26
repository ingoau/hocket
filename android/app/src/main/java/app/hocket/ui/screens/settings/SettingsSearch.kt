package app.hocket.ui.screens.settings

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SegmentedListItem
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.foundation.ScrollState
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInParent
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import app.hocket.R
import app.hocket.ui.components.segmentedListColors
import app.hocket.ui.icons.HocketIcons
import kotlinx.coroutines.delay

/**
 * One setting a person might search for: its row's stable [tag] (see [settingTag]), the category
 * whose screen it is on, and the strings it is found by — the row's own [label] and [body] (so the
 * index reads the way the page does) and [keywords], the words people search with that the labels
 * don't use ("dark mode", "bass").
 */
data class SettingEntry(val tag: String, val category: SettingsCategory, val label: Int, val body: Int? = null, val keywords: Int? = null)

/** Every searchable setting, in the order the categories list them. */
val SETTINGS_INDEX: List<SettingEntry> = listOf(
    SettingEntry("server.syncNow", SettingsCategory.Account, R.string.settings_sync_now, keywords = R.string.settings_search_kw_server),
    SettingEntry("server.fullSync", SettingsCategory.Account, R.string.settings_full_sync, keywords = R.string.settings_search_kw_server),
    SettingEntry("sync.master", SettingsCategory.Account, R.string.settings_sync_master, R.string.settings_sync_master_body),
    SettingEntry("server.remove", SettingsCategory.Account, R.string.settings_remove_server, keywords = R.string.settings_search_kw_server),
    SettingEntry("display.theme", SettingsCategory.Appearance, R.string.settings_theme, keywords = R.string.settings_search_kw_theme),
    SettingEntry("display.accent", SettingsCategory.Appearance, R.string.settings_accent, keywords = R.string.settings_search_kw_accent),
    SettingEntry("display.artworkColour", SettingsCategory.Appearance, R.string.settings_accent_artwork, keywords = R.string.settings_search_kw_accent),
    SettingEntry("display.animatedBackground", SettingsCategory.Appearance, R.string.settings_animated_background, R.string.settings_animated_background_body),
    SettingEntry("display.immersiveArtwork", SettingsCategory.Appearance, R.string.settings_immersive, R.string.settings_immersive_body, R.string.settings_search_kw_artwork),
    SettingEntry("display.playerRating", SettingsCategory.Appearance, R.string.settings_player_rating, R.string.settings_player_rating_body, R.string.settings_search_kw_rating),
    SettingEntry("queue.mode", SettingsCategory.Playback, R.string.queue_mode, keywords = R.string.settings_search_kw_queue),
    SettingEntry("queue.savedCap", SettingsCategory.Playback, R.string.settings_saved_cap, keywords = R.string.settings_search_kw_queue),
    SettingEntry("queue.autoplay", SettingsCategory.Playback, R.string.settings_autoplay, keywords = R.string.settings_search_kw_queue),
    SettingEntry("sleep.defaultMinutes", SettingsCategory.Playback, R.string.settings_sleep_default, keywords = R.string.settings_search_kw_sleep),
    SettingEntry("sleep.stopAtEndOfTrack", SettingsCategory.Playback, R.string.sleep_end_of_track, keywords = R.string.settings_search_kw_sleep),
    SettingEntry("media.externalControl", SettingsCategory.Playback, R.string.settings_external_control, R.string.settings_external_control_summary, R.string.settings_search_kw_media),
    SettingEntry("widget.add", SettingsCategory.Playback, R.string.settings_widget_add, keywords = R.string.settings_search_kw_widget),
    SettingEntry("home.shortcuts", SettingsCategory.Playback, R.string.settings_shortcuts, keywords = R.string.settings_search_kw_widget),
    SettingEntry("audio.replayGain", SettingsCategory.Audio, R.string.settings_replay_gain, keywords = R.string.settings_search_kw_volume),
    SettingEntry("audio.preamp", SettingsCategory.Audio, R.string.settings_preamp, keywords = R.string.settings_search_kw_volume),
    SettingEntry("audio.normalisation", SettingsCategory.Audio, R.string.settings_normalisation, keywords = R.string.settings_search_kw_volume),
    SettingEntry("audio.gapless", SettingsCategory.Audio, R.string.settings_gapless, keywords = R.string.settings_search_kw_gapless),
    SettingEntry("audio.eq", SettingsCategory.Audio, R.string.settings_eq, keywords = R.string.settings_search_kw_eq),
    SettingEntry("audio.systemEq", SettingsCategory.Audio, R.string.settings_system_eq, R.string.settings_system_eq_body, R.string.settings_search_kw_eq),
    SettingEntry("audio.output", SettingsCategory.Audio, R.string.settings_output_device, keywords = R.string.settings_search_kw_output),
    SettingEntry("transcoding.format", SettingsCategory.Streaming, R.string.settings_format, keywords = R.string.settings_search_kw_transcoding),
    SettingEntry("transcoding.bitrate", SettingsCategory.Streaming, R.string.settings_max_bitrate, keywords = R.string.settings_search_kw_transcoding),
    SettingEntry("transcoding.cannotDecode", SettingsCategory.Streaming, R.string.settings_cannot_decode, keywords = R.string.settings_search_kw_transcoding),
    SettingEntry("open.downloads", SettingsCategory.Downloads, R.string.nav_downloads, keywords = R.string.settings_search_kw_storage),
    SettingEntry("downloads.wifiOnly", SettingsCategory.Downloads, R.string.settings_downloads_wifi_only, R.string.settings_downloads_wifi_only_body, R.string.settings_search_kw_data),
    SettingEntry("storage.warnThreshold", SettingsCategory.Downloads, R.string.downloads_threshold, keywords = R.string.settings_search_kw_storage),
    SettingEntry("storage.cacheUsage", SettingsCategory.Downloads, R.string.settings_cache_in_use, keywords = R.string.settings_search_kw_cache),
    SettingEntry("storage.cacheMaxBytes", SettingsCategory.Downloads, R.string.settings_cache_budget, R.string.settings_cache_budget_body, R.string.settings_search_kw_cache),
    SettingEntry("storage.prefetchOnMobileData", SettingsCategory.Downloads, R.string.settings_prefetch_mobile, R.string.settings_prefetch_mobile_body, R.string.settings_search_kw_data),
    SettingEntry("storage.dataSaved", SettingsCategory.Downloads, R.string.settings_data_saved, keywords = R.string.settings_search_kw_data),
    SettingEntry("storage.clearCache", SettingsCategory.Downloads, R.string.downloads_clear_cache, keywords = R.string.settings_search_kw_cache),
    SettingEntry("lyrics.external", SettingsCategory.Lyrics, R.string.settings_external_lyrics, R.string.settings_external_lyrics_privacy, R.string.settings_search_kw_lyrics),
    SettingEntry("lyrics.defaultOffsetMs", SettingsCategory.Lyrics, R.string.settings_lyrics_default_offset, keywords = R.string.settings_search_kw_lyrics),
    SettingEntry("lyrics.showTranslations", SettingsCategory.Lyrics, R.string.settings_lyrics_translations, keywords = R.string.settings_search_kw_lyrics),
    SettingEntry("ratings.loveThreshold", SettingsCategory.Library, R.string.settings_love_threshold, keywords = R.string.settings_search_kw_rating),
    SettingEntry("open.filters", SettingsCategory.Library, R.string.nav_filters),
    SettingEntry("open.stats", SettingsCategory.Library, R.string.nav_stats),
    SettingEntry("battery.saver", SettingsCategory.Battery, R.string.settings_battery_saver, R.string.settings_battery_body, R.string.settings_search_kw_battery),
    SettingEntry("battery.autoEngage", SettingsCategory.Battery, R.string.settings_battery_auto, keywords = R.string.settings_search_kw_battery),
    SettingEntry("connect.coordinator", SettingsCategory.Connect, R.string.settings_coordinator_url, keywords = R.string.settings_search_kw_connect),
    SettingEntry("connect.lan", SettingsCategory.Connect, R.string.settings_lan_discovery, keywords = R.string.settings_search_kw_connect),
    SettingEntry("pebble.watchapp", SettingsCategory.Pebble, R.string.pebble_watchapp, keywords = R.string.settings_search_kw_pebble),
    SettingEntry("pebble.touchNavigation", SettingsCategory.Pebble, R.string.pebble_touch_navigation, keywords = R.string.settings_search_kw_pebble),
    SettingEntry("pebble.buttonsOnly", SettingsCategory.Pebble, R.string.pebble_buttons_only, keywords = R.string.settings_search_kw_pebble),
    SettingEntry("pebble.menuTimeoutSeconds", SettingsCategory.Pebble, R.string.pebble_menu_timeout, keywords = R.string.settings_search_kw_pebble),
    SettingEntry("pebble.showVolumeControls", SettingsCategory.Pebble, R.string.pebble_volume_controls, keywords = R.string.settings_search_kw_pebble),
    SettingEntry("pebble.showProgressBar", SettingsCategory.Pebble, R.string.pebble_progress_bar, keywords = R.string.settings_search_kw_pebble),
    SettingEntry("pebble.showAlbumArt", SettingsCategory.Pebble, R.string.pebble_album_art, keywords = R.string.settings_search_kw_pebble),
    SettingEntry("pebble.autoOpen", SettingsCategory.Pebble, R.string.pebble_auto_open, keywords = R.string.settings_search_kw_pebble),
    SettingEntry("pebble.closeWhenPausedMinutes", SettingsCategory.Pebble, R.string.pebble_close_when_paused, keywords = R.string.settings_search_kw_pebble),
    SettingEntry("customise.bottomBar", SettingsCategory.Customise, R.string.settings_bottom_bar, keywords = R.string.settings_search_kw_customise),
    SettingEntry("customise.contextMenuSection", SettingsCategory.Customise, R.string.settings_context_menu, keywords = R.string.settings_search_kw_customise),
    SettingEntry("customise.mediaSessionSection", SettingsCategory.Customise, R.string.settings_media_buttons, keywords = R.string.settings_search_kw_media),
    SettingEntry("customise.swipe", SettingsCategory.Customise, R.string.settings_swipe_actions, keywords = R.string.settings_search_kw_customise),
    SettingEntry("backup.includeSecrets", SettingsCategory.Backup, R.string.settings_export_with_secrets, keywords = R.string.settings_search_kw_backup),
    SettingEntry("backup.export", SettingsCategory.Backup, R.string.settings_export_config, keywords = R.string.settings_search_kw_backup),
    SettingEntry("backup.import", SettingsCategory.Backup, R.string.settings_import_config, keywords = R.string.settings_search_kw_backup),
    SettingEntry("backup.diagnostics", SettingsCategory.Backup, R.string.settings_copy_diagnostics, keywords = R.string.settings_search_kw_diagnostics),
    SettingEntry("developer.mode", SettingsCategory.Advanced, R.string.settings_developer_mode, R.string.settings_developer_mode_body, R.string.settings_search_kw_diagnostics),
    SettingEntry("developer.open", SettingsCategory.Advanced, R.string.settings_developer_diagnostics, R.string.settings_developer_diagnostics_body, R.string.settings_search_kw_diagnostics),
)

/** Keywords for a whole category, so "watch" finds Pebble and "logs" finds Backup & diagnostics. */
private val CATEGORY_KEYWORDS: Map<SettingsCategory, Int> = mapOf(
    SettingsCategory.Account to R.string.settings_search_kw_server,
    SettingsCategory.Streaming to R.string.settings_search_kw_transcoding,
    SettingsCategory.Downloads to R.string.settings_search_kw_storage,
    SettingsCategory.Connect to R.string.settings_search_kw_connect,
    SettingsCategory.Pebble to R.string.settings_search_kw_pebble,
    SettingsCategory.Customise to R.string.settings_search_kw_customise,
    SettingsCategory.Backup to R.string.settings_search_kw_backup,
    SettingsCategory.Advanced to R.string.settings_search_kw_diagnostics,
    SettingsCategory.About to R.string.settings_search_kw_about,
)

/** A search result: a whole category ([tag] null) or one setting on its screen. */
data class SettingsHit(val category: SettingsCategory, val label: String, val tag: String? = null)

/** Lower case without accents, so "equaliser" and "Égaliseur" compare plainly. */
private fun fold(s: String): String = java.text.Normalizer.normalize(s, java.text.Normalizer.Form.NFD).replace(Regex("\\p{Mn}+"), "").lowercase()

/**
 * Every word of [query] must appear in the entry (its label, body, keywords or category name). A
 * label that starts with the query ranks first, then a label holding every word, then the rest; a
 * category comes before its settings at the same rank. [str] resolves a string resource and
 * [categoryTitle] names a category. Empty for a blank query.
 */
fun searchSettings(query: String, str: (Int) -> String, categoryTitle: (SettingsCategory) -> String): List<SettingsHit> {
    val words = fold(query).split(Regex("\\s+")).filter { it.isNotEmpty() }
    if (words.isEmpty()) return emptyList()
    val q = words.joinToString(" ")
    data class Scored(val hit: SettingsHit, val score: Int, val order: Int)
    val out = mutableListOf<Scored>()
    fun consider(hit: SettingsHit, haystack: String, order: Int) {
        val label = fold(hit.label)
        val all = label + " " + fold(haystack)
        if (!words.all { it in all }) return
        val score = if (label.startsWith(q)) 0 else if (words.all { it in label }) 1 else 2
        out += Scored(hit, score, order)
    }
    SettingsCategory.entries.forEachIndexed { i, c ->
        consider(SettingsHit(c, categoryTitle(c)), CATEGORY_KEYWORDS[c]?.let(str).orEmpty(), i * 100)
    }
    SETTINGS_INDEX.forEachIndexed { i, e ->
        val haystack = listOfNotNull(e.body?.let(str), e.keywords?.let(str), categoryTitle(e.category)).joinToString(" ")
        consider(SettingsHit(e.category, str(e.label), e.tag), haystack, e.category.ordinal * 100 + 1 + i)
    }
    return out.sortedWith(compareBy({ it.score }, { it.order })).map { it.hit }
}

/**
 * The setting a search picked, for the screen it opens: set just before navigating, taken (once) by
 * that screen's [SubScreen], which scrolls to the row and highlights it.
 */
object SettingsJump {
    private var pending: String? = null
    fun request(tag: String?) { pending = tag }
    fun take(): String? = pending.also { pending = null }
}

/** The picked row on a settings screen: where it sits once laid out, and whether it is still highlighted. */
@Stable
class JumpTarget(val tag: String) {
    var y by mutableStateOf<Float?>(null)
    var highlighted by mutableStateOf(true)
    val focus = FocusRequester()
}

val LocalJumpTarget = compositionLocalOf<JumpTarget?> { null }

/**
 * Marks the row whose stable id is [tag] as a jump target: when it is the one a search picked, it
 * reports where it is (for the screen to scroll to), takes focus, and is tinted for a moment.
 */
@Composable
fun Modifier.settingAnchor(tag: String?): Modifier {
    val target = LocalJumpTarget.current
    if (tag == null || target == null || target.tag != tag) return this
    val tint by animateColorAsState(if (target.highlighted) MaterialTheme.colorScheme.primary.copy(alpha = 0.14f) else Color.Transparent, tween(600), label = "settingHighlight")
    return this.onGloballyPositioned { if (target.y == null) target.y = it.positionInParent().y }
        .background(tint, MaterialTheme.shapes.large)
        .focusRequester(target.focus)
}

/** Scrolls [scroll] to the picked row once it is laid out, focuses it, and fades the highlight. */
@Composable
internal fun JumpEffect(target: JumpTarget?, scroll: ScrollState) {
    target ?: return
    val margin = with(LocalDensity.current) { 96.dp.toPx() }
    val y = target.y
    LaunchedEffect(y) {
        if (y == null) return@LaunchedEffect
        scroll.animateScrollTo((y - margin).toInt().coerceAtLeast(0))
        runCatching { target.focus.requestFocus() }
        delay(HIGHLIGHT_MS)
        target.highlighted = false
    }
}

private const val HIGHLIGHT_MS = 1_800L

/** The M3 search bar at the top of Settings: a full-round 56dp field on the highest container. */
@Composable
internal fun SettingsSearchField(query: String, onQuery: (String) -> Unit, onSubmit: () -> Unit, modifier: Modifier = Modifier) {
    TextField(
        value = query, onValueChange = onQuery, singleLine = true, placeholder = { Text(stringResource(R.string.settings_search_hint)) },
        leadingIcon = { Icon(HocketIcons.Filled.Search, null) },
        trailingIcon = { if (query.isNotEmpty()) IconButton(onClick = { onQuery("") }, modifier = Modifier.testTag("settings.search.clear")) { Icon(HocketIcons.Filled.Close, stringResource(R.string.settings_search_clear)) } },
        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
        keyboardActions = KeyboardActions(onSearch = { onSubmit() }),
        shape = CircleShape,
        colors = TextFieldDefaults.colors(
            focusedContainerColor = MaterialTheme.colorScheme.surfaceContainerHigh, unfocusedContainerColor = MaterialTheme.colorScheme.surfaceContainerHigh,
            focusedIndicatorColor = Color.Transparent, unfocusedIndicatorColor = Color.Transparent,
        ),
        modifier = modifier.fillMaxWidth().heightIn(min = 56.dp).testTag("settings.search"),
    )
}

/** The matches for a query, as one segmented group naming each one's category; or the empty state. */
@Composable
internal fun SettingsSearchResults(hits: List<SettingsHit>, query: String, onPick: (SettingsHit) -> Unit) {
    val count = pluralStringResource(R.plurals.settings_search_count, hits.size, hits.size)
    if (hits.isEmpty()) {
        Column(Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 16.dp).testTag("settings.search.empty").semantics { liveRegion = LiveRegionMode.Polite }, verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(stringResource(R.string.settings_search_none, query.trim()), style = MaterialTheme.typography.titleMedium)
            Text(stringResource(R.string.settings_search_none_hint), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        return
    }
    Column(Modifier.testTag("settings.search.results"), verticalArrangement = Arrangement.spacedBy(ListItemDefaults.SegmentedGap)) {
        Text(count, style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.primary, modifier = Modifier.padding(start = 8.dp, bottom = 6.dp).semantics { liveRegion = LiveRegionMode.Polite })
        val section = stringResource(R.string.settings_search_section)
        hits.forEachIndexed { i, h ->
            SegmentedListItem(
                onClick = { onPick(h) },
                shapes = ListItemDefaults.segmentedShapes(i, hits.size),
                colors = segmentedListColors(),
                supportingContent = { Text(if (h.tag == null) section else h.category.title()) },
                modifier = Modifier.testTag(if (h.tag == null) "settings.hit.category.${h.category.id}" else "settings.hit.${h.tag}"),
            ) { Text(h.label) }
        }
    }
}

/** [searchSettings] with the app's strings. */
@Composable
internal fun rememberSettingsHits(query: String): List<SettingsHit> {
    val resources = LocalResources.current
    val titles = SettingsCategory.entries.associateWith { it.title() }
    return remember(query, resources) { searchSettings(query, { resources.getString(it) }, { titles.getValue(it) }) }
}
