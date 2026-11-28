package app.hocket.ui.screens.settings

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.ButtonGroupDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.TextButton
import androidx.compose.material3.ToggleButton
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import app.hocket.core.MenuItemIds
import app.hocket.core.SettingKeys
import app.hocket.core.api.MenuItemInfo
import app.hocket.core.api.MenuSection
import app.hocket.core.api.MenuTier
import app.hocket.ui.icons.HocketIcons
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.navigation.NavHostController
import app.hocket.R
import app.hocket.core.ActionIds
import app.hocket.core.Commands
import app.hocket.core.SwipeOptions
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.components.SwipeSurface
import app.hocket.ui.nav.BottomBarEditor
import app.hocket.ui.nav.icon
import app.hocket.ui.nav.label
import app.hocket.ui.nav.navItems

/*
 * The settings' own sub-pages (Route.SettingsPage): the editors that were too long to sit inline
 * on their category's screen. Each is a [SubScreen] like the categories, opened from a row there.
 */

internal const val PAGE_BOTTOM_BAR = "customise.bottomBar"
internal const val PAGE_ITEM_MENU = "customise.itemMenu"
internal const val PAGE_MEDIA_BUTTONS = "customise.mediaSession"
internal const val PAGE_SWIPE = "customise.swipe"
private const val PAGE_TRANSCODING = "transcoding."

/** The page [page] names (one of the PAGE_ ids, a Pebble list's setting key, or `transcoding.<networkId>`). */
@Composable
fun SettingsPageScreen(nav: NavHostController, page: String) {
    when {
        page == PAGE_BOTTOM_BAR -> BottomBarPage(nav)
        page == PAGE_ITEM_MENU -> ItemMenuPage(nav)
        page == PAGE_MEDIA_BUTTONS -> MediaButtonsPage(nav)
        page == PAGE_SWIPE -> SwipePage(nav)
        page.startsWith(PAGE_TRANSCODING) -> TranscodingProfileScreen(nav, page.removePrefix(PAGE_TRANSCODING))
        else -> PebbleItemsPage(nav, page)
    }
}

/** The bottom bar: a picture of the bar as it will look, then its editor. */
@Composable
private fun BottomBarPage(nav: NavHostController) {
    SubScreen(nav, stringResource(R.string.settings_bottom_bar)) {
        val items = navItems()
        BottomBarPreview(items.map { it.icon(false) to it.label() }, Modifier.padding(horizontal = 16.dp, vertical = 12.dp).testTag("customise.bottomBar.preview"))
        BottomBarEditor(Modifier.testTag("customise.bottomBar.editor"))
    }
}

/**
 * The item menus' rows (`Query.MenuItems`), grouped by section in menu order: each Shown, Under
 * More or Hidden (only the tiers it can take), then Reset. The play strip isn't customisable.
 */
@Composable
private fun ItemMenuPage(nav: NavHostController) {
    val client = LocalCoreClient.current
    SubScreen(nav, stringResource(R.string.settings_item_menus)) {
        // Asked again whenever the stored customisation changes (here, or on another device).
        val stored = setting(SettingKeys.MENU_OVERRIDES).raw
        val items by produceState<List<MenuItemInfo>?>(null, stored) { value = client.menuItems() }
        Text(stringResource(R.string.settings_item_menus_hint), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp))
        items?.let { list ->
            MenuItemsEditor(list, tag = "customise.itemMenu", onSet = { id, tier -> client.dispatch(Commands.setMenuItem(id, tier)) }, onReset = { client.dispatch(Commands.resetMenuItems()) })
        }
    }
}

/** [items] by section, each row's tier a connected choice; tags `<tag>.<id>` and `<tag>.<id>.<tier>`, `<tag>.reset`. */
@Composable
internal fun MenuItemsEditor(items: List<MenuItemInfo>, tag: String, onSet: (String, MenuTier) -> Unit, onReset: () -> Unit) {
    // What this page last chose, shown until the stored value comes back.
    var shown by remember(items) { mutableStateOf(items.associate { it.id to it.tier }) }
    Column(Modifier.fillMaxWidth().testTag(tag)) {
        items.groupBy { it.section }.toSortedMap().forEach { (section, rows) ->
            SettingsSection(stringResource(menuSectionTitle(section)))
            rows.forEach { item ->
                val name = menuItemLabel(item.id)
                Row(Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 8.dp, bottom = 8.dp).testTag("$tag.${item.id}"), verticalAlignment = Alignment.CenterVertically) {
                    Icon(menuItemIcon(item.id), null, tint = MaterialTheme.colorScheme.primary)
                    Spacer(Modifier.width(16.dp))
                    Column(Modifier.weight(1f)) {
                        Text(name, style = MaterialTheme.typography.bodyLarge)
                        TierChoice(item.choices, shown[item.id] ?: item.tier, name, "$tag.${item.id}", Modifier.padding(top = 6.dp)) { tier ->
                            shown = shown + (item.id to tier)
                            onSet(item.id, tier)
                        }
                    }
                }
            }
        }
        val customised = items.any { (shown[it.id] ?: it.tier) != it.defaultTier }
        TextButton(onClick = { shown = items.associate { it.id to it.defaultTier }; onReset() }, enabled = customised, modifier = Modifier.padding(horizontal = 8.dp, vertical = 8.dp).testTag("$tag.reset")) {
            Text(stringResource(R.string.bottom_bar_reset))
        }
    }
}

/** A row's tiers as connected toggle buttons (Shown | Under More | Hidden), one of them on. */
@Composable
private fun TierChoice(choices: List<MenuTier>, tier: MenuTier, name: String, tag: String, modifier: Modifier = Modifier, onPick: (MenuTier) -> Unit) {
    Row(modifier.semantics { contentDescription = name }, horizontalArrangement = Arrangement.spacedBy(ButtonGroupDefaults.ConnectedSpaceBetween)) {
        choices.forEachIndexed { i, t ->
            val shapes = when (i) {
                0 -> ButtonGroupDefaults.connectedLeadingButtonShapes()
                choices.lastIndex -> ButtonGroupDefaults.connectedTrailingButtonShapes()
                else -> ButtonGroupDefaults.connectedMiddleButtonShapes()
            }
            ToggleButton(checked = t == tier, onCheckedChange = { if (t != tier) onPick(t) }, shapes = shapes,
                modifier = Modifier.weight(1f).semantics { role = Role.RadioButton }.testTag("$tag.${t.string}")) {
                Text(stringResource(tierLabel(t)), maxLines = 1, style = MaterialTheme.typography.labelLarge)
            }
        }
    }
}

private fun tierLabel(t: MenuTier) = when (t) {
    MenuTier.Shown -> R.string.settings_menu_tier_shown
    MenuTier.More -> R.string.settings_menu_tier_more
    MenuTier.Hidden -> R.string.settings_menu_tier_hidden
}

private fun menuSectionTitle(s: MenuSection) = when (s) {
    MenuSection.Play -> R.string.menu_play_group
    MenuSection.Rate -> R.string.settings_menu_section_rate
    MenuSection.Organise -> R.string.settings_menu_section_organise
    MenuSection.GoTo -> R.string.settings_menu_section_go_to
    MenuSection.Keep -> R.string.settings_menu_section_keep
    MenuSection.About -> R.string.settings_menu_section_about
    MenuSection.Tune -> R.string.settings_menu_section_tune
    MenuSection.Danger -> R.string.settings_menu_section_danger
}

/** A menu row's name in the editor: its own words for the rows that aren't one action, else the action's. */
@Composable
internal fun menuItemLabel(id: String): String = when (id) {
    MenuItemIds.RATING -> stringResource(R.string.menu_item_rating)
    MenuItemIds.SHARE -> stringResource(R.string.menu_item_share)
    MenuItemIds.INFO -> stringResource(R.string.menu_item_info)
    MenuItemIds.GENIUS -> stringResource(R.string.menu_item_genius)
    MenuItemIds.LYRICS_TIMING -> stringResource(R.string.menu_item_lyrics_timing)
    ActionIds.PIN_TO_LIBRARY -> stringResource(R.string.menu_item_pin_to_library)
    ActionIds.ADD_TO_LIBRARY -> stringResource(R.string.menu_item_add_to_library)
    else -> actionLabel(id)
}

internal fun menuItemIcon(id: String): androidx.compose.ui.graphics.vector.ImageVector = when (id) {
    MenuItemIds.RATING -> HocketIcons.Filled.Star
    MenuItemIds.SHARE -> HocketIcons.Filled.Share
    MenuItemIds.INFO -> HocketIcons.Filled.Info
    MenuItemIds.GENIUS -> HocketIcons.Filled.Lyrics
    MenuItemIds.LYRICS_TIMING -> HocketIcons.Filled.Tune
    ActionIds.ADD_TO_LIBRARY -> HocketIcons.Filled.LibraryMusic
    else -> actionIdIcon(id)
}

/** The notification's buttons: a picture of the notification with them, Android's limit, then the list. */
@Composable
private fun MediaButtonsPage(nav: NavHostController) {
    val client = LocalCoreClient.current
    SubScreen(nav, stringResource(R.string.settings_media_buttons)) {
        val ids = mediaSessionIds()
        MediaNotificationPreview(ids, Modifier.padding(horizontal = 16.dp, vertical = 12.dp).testTag("customise.mediaSession.preview"))
        Text(stringResource(R.string.settings_media_buttons_hint), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp))
        ChooseAndOrder(ActionIds.MEDIA_SESSION_CHOICES, ids, minEnabled = 0, label = { actionLabel(it) }, icon = ::actionIdIcon,
            default = ActionIds.MEDIA_SESSION, tag = "customise.mediaSession") { next -> client.dispatch(Commands.setActionOrder("mediaSession", next)) }
    }
}

@Composable
private fun SwipePage(nav: NavHostController) {
    SubScreen(nav, stringResource(R.string.settings_swipe_actions)) {
        SwipeChoices(stringResource(R.string.settings_swipe_queue), SwipeOptions.QUEUE, SwipeSurface.Queue)
        SwipeChoices(stringResource(R.string.settings_swipe_lists), SwipeOptions.LIST, SwipeSurface.List)
        SwipeChoices(stringResource(R.string.settings_swipe_stash), SwipeOptions.STASH, SwipeSurface.Stash)
    }
}
