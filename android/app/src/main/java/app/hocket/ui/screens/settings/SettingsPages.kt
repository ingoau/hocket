package app.hocket.ui.screens.settings

import androidx.compose.foundation.layout.padding
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
internal const val PAGE_CONTEXT_MENU = "customise.contextMenu"
internal const val PAGE_MEDIA_BUTTONS = "customise.mediaSession"
internal const val PAGE_SWIPE = "customise.swipe"
private const val PAGE_TRANSCODING = "transcoding."

/** The page [page] names (one of the PAGE_ ids, a Pebble list's setting key, or `transcoding.<networkId>`). */
@Composable
fun SettingsPageScreen(nav: NavHostController, page: String) {
    when {
        page == PAGE_BOTTOM_BAR -> BottomBarPage(nav)
        page == PAGE_CONTEXT_MENU -> ContextMenuPage(nav)
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

@Composable
private fun ContextMenuPage(nav: NavHostController) {
    val client = LocalCoreClient.current
    SubScreen(nav, stringResource(R.string.settings_context_menu)) {
        ChooseAndOrder(ActionIds.CONTEXT_MENU, contextMenuIds(), minEnabled = 1, label = { actionLabel(it) }, icon = ::actionIdIcon,
            default = ActionIds.CONTEXT_MENU, tag = "customise.contextMenu") { ids -> client.dispatch(Commands.setActionOrder("contextMenu", ids)) }
    }
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
    }
}
