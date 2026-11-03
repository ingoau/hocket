package app.hocket.ui.nav

import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import app.hocket.R
import app.hocket.ui.a11y.LocalReducedMotion
import app.hocket.ui.icons.HocketIcons
import app.hocket.ui.lyrics.LyricsPage
import app.hocket.ui.player.PlayerMode
import app.hocket.ui.queue.QueueList

/**
 * Beside the open side panel (landscape tablets), the panel's Lyrics and Queue buttons take over the
 * main screen: the lyrics in a readable column, or the queue at full size, over the page (which
 * stays as it was underneath). Back, the close button or a place in the bar hands the screen back
 * to the page ([onClose]). Null [page]: nothing shown.
 */
@Composable
internal fun PanelPageHost(page: PlayerMode?, onClose: () -> Unit) {
    val reduced = LocalReducedMotion.current
    // The page last shown, so it stays while it fades away.
    var held by remember { mutableStateOf(page) }
    if (page != null) held = page
    BackHandler(enabled = page != null, onBack = onClose)
    AnimatedVisibility(
        visible = page != null,
        enter = if (reduced) fadeIn(tween(0)) else fadeIn(tween(200)) + scaleIn(spring(dampingRatio = 1f, stiffness = 500f), initialScale = 0.97f),
        exit = if (reduced) fadeOut(tween(0)) else fadeOut(tween(150)),
    ) {
        val shown = held ?: return@AnimatedVisibility
        Surface(color = MaterialTheme.colorScheme.surface, modifier = Modifier.fillMaxSize().testTag("panelPage")) {
            Column(Modifier.fillMaxSize().statusBarsPadding()) {
                Row(Modifier.fillMaxWidth().padding(start = 24.dp, end = 12.dp, top = 12.dp, bottom = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        stringResource(if (shown == PlayerMode.Queue) R.string.player_mode_queue else R.string.player_mode_lyrics),
                        style = MaterialTheme.typography.headlineMedium,
                        fontWeight = FontWeight.Bold,
                        modifier = Modifier.weight(1f).semantics { heading() },
                    )
                    IconButton(onClick = onClose, modifier = Modifier.testTag("panelPage.close")) {
                        Icon(HocketIcons.Filled.Close, stringResource(R.string.panel_close))
                    }
                }
                // Lines and rows stay a readable width on a wide screen, centred.
                Box(Modifier.weight(1f).fillMaxWidth().padding(bottom = BottomContentInset), contentAlignment = Alignment.TopCenter) {
                    when (shown) {
                        PlayerMode.Queue -> QueueList(Modifier.widthIn(max = 760.dp).fillMaxSize().testTag("panelPage.queue"), contentPadding = PaddingValues(horizontal = 8.dp))
                        else -> LyricsPage(visible = page == PlayerMode.Lyrics, modifier = Modifier.widthIn(max = 680.dp).fillMaxSize().padding(horizontal = 8.dp).testTag("panelPage.lyrics"))
                    }
                }
            }
        }
    }
}
