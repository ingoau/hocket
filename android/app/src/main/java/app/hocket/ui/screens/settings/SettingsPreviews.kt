package app.hocket.ui.screens.settings

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.clipRect
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.hocket.core.ActionIds
import app.hocket.ui.components.SwipeLook
import app.hocket.ui.components.swipeLook
import androidx.compose.ui.geometry.RoundRect
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.compositeOver
import androidx.compose.ui.graphics.drawscope.clipPath
import app.hocket.ui.icons.HocketIcons
import kotlin.math.abs
import kotlin.math.sin

/*
 * The small pictures in the settings' option cards ([OptionCards]) and the previews over the
 * customise pages: each draws what a choice looks like, from the theme's colours, at a glance.
 * All decoration: the cards and rows around them carry the words.
 */

/** A light or a dark phone screen (or, for "system", one of each side by side): artwork and two lines of text. */
@Composable
fun ThemePreview(mode: String, selected: Boolean) {
    val outline = if (selected) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.outlineVariant
    val accent = MaterialTheme.colorScheme.primary
    Canvas(Modifier.size(width = 38.dp, height = 54.dp)) {
        val radius = CornerRadius(7.dp.toPx())
        fun phone(bg: Color, fg: Color, art: Color) {
            drawRoundRect(bg, cornerRadius = radius)
            val w = size.width
            drawRoundRect(art, Offset(w * 0.16f, size.height * 0.12f), Size(w * 0.68f, w * 0.68f), CornerRadius(3.dp.toPx()))
            drawRoundRect(fg, Offset(w * 0.16f, size.height * 0.66f), Size(w * 0.56f, size.height * 0.06f), CornerRadius(2.dp.toPx()))
            drawRoundRect(fg.copy(alpha = 0.5f), Offset(w * 0.16f, size.height * 0.77f), Size(w * 0.38f, size.height * 0.05f), CornerRadius(2.dp.toPx()))
        }
        val light = { phone(Color(0xFFFDF7FF), Color(0xFF1D1B20), accent.copy(alpha = 0.55f)) }
        val dark = { phone(Color(0xFF141218), Color(0xFFE6E0E9), accent.copy(alpha = 0.8f)) }
        when (mode) {
            "light" -> light()
            "dark" -> dark()
            else -> {
                clipRect(right = size.width / 2) { light() }
                clipRect(left = size.width / 2) { dark() }
            }
        }
        drawRoundRect(outline, cornerRadius = radius, style = Stroke(1.5.dp.toPx()))
    }
}

/** A stand-in cover: the theme's colours in a diagonal wash. */
private fun artBrush(a: Color, b: Color) = Brush.linearGradient(listOf(a, b))

/**
 * The full player: "always" runs the artwork edge to edge and carries it down the screen, "never"
 * sits it as a card, "automatic" shows one of each side by side (it picks per cover).
 */
@Composable
fun ImmersivePreview(mode: String, selected: Boolean) {
    val scheme = MaterialTheme.colorScheme
    val outline = if (selected) scheme.primary else scheme.outlineVariant
    val bg = scheme.surfaceContainer
    val fg = scheme.onSurface
    val a = scheme.primary
    val b = scheme.tertiary
    Canvas(Modifier.size(width = 38.dp, height = 54.dp)) {
        val radius = CornerRadius(7.dp.toPx())
        val w = size.width
        val h = size.height
        fun controls() {
            drawRoundRect(fg, Offset(w * 0.16f, h * 0.72f), Size(w * 0.5f, h * 0.05f), CornerRadius(2.dp.toPx()))
            drawCircle(fg, radius = w * 0.09f, center = Offset(w * 0.5f, h * 0.87f))
        }
        fun immersive() {
            drawRect(bg)
            // The artwork from edge to edge, melting into the page below it in its own colour.
            drawRect(artBrush(a, b), Offset.Zero, Size(w, h * 0.66f))
            drawRect(Brush.verticalGradient(0.3f to Color.Transparent, 0.66f to b.copy(alpha = 0.35f).compositeOver(bg), 1f to bg, endY = h), Offset.Zero, Size(w, h))
            controls()
        }
        fun card() {
            drawRect(bg)
            drawRoundRect(artBrush(a, b), Offset(w * 0.16f, h * 0.14f), Size(w * 0.68f, w * 0.68f), CornerRadius(4.dp.toPx()))
            controls()
        }
        // Everything inside the phone's rounded outline, so the artwork follows its corners.
        val screen = Path().apply { addRoundRect(RoundRect(0f, 0f, w, h, radius)) }
        clipPath(screen) {
            when (mode) {
                "always" -> immersive()
                "never" -> card()
                else -> {
                    clipRect(right = w / 2) { immersive() }
                    clipRect(left = w / 2) { card() }
                }
            }
        }
        drawRoundRect(outline, cornerRadius = radius, style = Stroke(1.5.dp.toPx()))
    }
}

/**
 * Two songs meeting, as two decks: the outgoing song on top, the next one below. "off" ends one
 * and then starts the other, "crossfade" shrinks one as the other grows, "automix" runs both at
 * full level through the overlap, on a shared beat.
 */
@Composable
fun TransitionPreview(mode: String, selected: Boolean) {
    val scheme = MaterialTheme.colorScheme
    val first = if (selected) scheme.primary else scheme.onSurfaceVariant
    val second = if (selected) scheme.tertiary else scheme.outline
    val beat = scheme.onSurfaceVariant.copy(alpha = 0.45f)
    Canvas(Modifier.fillMaxSize().padding(horizontal = 10.dp, vertical = 10.dp)) {
        val bars = 16
        val step = size.width / bars
        val lane = size.height / 2
        // How loud each song is across the width (x is 0..1).
        fun level(x: Float, isFirst: Boolean): Float = when (mode) {
            "off" -> if (isFirst) (if (x < 0.5f) 1f else 0f) else (if (x >= 0.5f) 1f else 0f)
            "crossfade" -> if (isFirst) ((0.8f - x) / 0.6f).coerceIn(0f, 1f) else ((x - 0.2f) / 0.6f).coerceIn(0f, 1f)
            else -> if (isFirst) (if (x < 0.75f) 1f else 0f) else (if (x >= 0.25f) 1f else 0f)
        }
        if (mode == "automix") {
            // The shared beat through the overlap.
            for (i in 0..2) {
                val x = size.width * (0.31f + i * 0.19f)
                drawLine(beat, Offset(x, 0f), Offset(x, size.height), strokeWidth = 1.dp.toPx())
            }
        }
        for (song in 0..1) {
            val isFirst = song == 0
            val centre = lane * (song + 0.5f)
            for (i in 0 until bars) {
                val x = (i + 0.5f) / bars
                val l = level(x, isFirst)
                if (l <= 0.05f) continue
                val wave = 0.55f + 0.45f * abs(sin(i * (if (isFirst) 1.3f else 1.9f) + song))
                val barH = (lane * 0.9f * l * wave).coerceAtLeast(2.dp.toPx())
                drawRoundRect(if (isFirst) first else second, Offset(i * step + step * 0.2f, centre - barH / 2), Size(step * 0.6f, barH), CornerRadius(step * 0.3f))
            }
        }
    }
}

/** Battery saver's levels as leaves: one, two, three; custom is the tuning sliders. */
@Composable
fun SaverPreview(leaves: Int, selected: Boolean) {
    val tint = if (selected) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant
    if (leaves == 0) { Icon(HocketIcons.Filled.Tune, null, tint = tint, modifier = Modifier.size(26.dp)); return }
    Row(horizontalArrangement = Arrangement.spacedBy((-2).dp)) {
        repeat(leaves) { Icon(HocketIcons.Filled.Eco, null, tint = tint, modifier = Modifier.size(22.dp)) }
    }
}

/**
 * A song row with both its swipes showing: what swiping right reveals on the left, what swiping
 * left reveals on the right (mirrored in right-to-left layouts, as the row is). A side with no
 * action stays the row's own colour.
 */
@Composable
fun SwipeRowPreview(startToEnd: String, endToStart: String, modifier: Modifier = Modifier) {
    val scheme = MaterialTheme.colorScheme
    val start = swipeLook(startToEnd)
    val end = swipeLook(endToStart)
    Box(modifier.fillMaxWidth().height(64.dp).clip(RoundedCornerShape(16.dp)).clearAndSetSemantics { }) {
        // Each side's colour runs under the row to the middle, filling in round its corners.
        Row(Modifier.fillMaxSize()) {
            Box(Modifier.weight(1f).fillMaxHeight().background(start?.container ?: scheme.surfaceContainerHighest))
            Box(Modifier.weight(1f).fillMaxHeight().background(end?.container ?: scheme.surfaceContainerHighest))
        }
        Row(Modifier.fillMaxSize()) {
            SwipeSide(start)
            Row(
                Modifier.weight(1f).fillMaxHeight().clip(RoundedCornerShape(12.dp)).background(scheme.surfaceContainerLow).padding(horizontal = 12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Box(Modifier.size(40.dp).clip(RoundedCornerShape(8.dp)).background(artBrush(scheme.primary, scheme.tertiary)))
                Spacer(Modifier.width(12.dp))
                Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    Box(Modifier.fillMaxWidth(0.7f).height(8.dp).clip(CircleShape).background(scheme.onSurface.copy(alpha = 0.8f)))
                    Box(Modifier.fillMaxWidth(0.45f).height(6.dp).clip(CircleShape).background(scheme.onSurfaceVariant.copy(alpha = 0.6f)))
                }
            }
            SwipeSide(end)
        }
    }
}

@Composable
private fun SwipeSide(look: SwipeLook?) {
    Box(
        Modifier.width(64.dp).fillMaxHeight(),
        contentAlignment = Alignment.Center,
    ) { if (look != null) Icon(look.icon, null, tint = look.content) }
}

/**
 * The media notification as Android draws it: the cover, two lines of text, and the buttons in
 * order. Android fits about two extra buttons beside play and skip: the rest are drawn faded.
 */
@Composable
fun MediaNotificationPreview(buttons: List<String>, modifier: Modifier = Modifier) {
    val scheme = MaterialTheme.colorScheme
    Column(
        modifier.fillMaxWidth().clip(RoundedCornerShape(24.dp)).background(scheme.surfaceContainerHigh).padding(16.dp).clearAndSetSemantics { },
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.size(44.dp).clip(RoundedCornerShape(10.dp)).background(artBrush(scheme.primary, scheme.tertiary)))
            Spacer(Modifier.width(12.dp))
            Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                Box(Modifier.width(120.dp).height(9.dp).clip(CircleShape).background(scheme.onSurface.copy(alpha = 0.8f)))
                Box(Modifier.width(80.dp).height(7.dp).clip(CircleShape).background(scheme.onSurfaceVariant.copy(alpha = 0.6f)))
            }
        }
        // Transport first, then the chosen extras in order.
        val transport = listOf(ActionIds.PREVIOUS, ActionIds.TOGGLE_PLAY, ActionIds.NEXT).filter { it == ActionIds.TOGGLE_PLAY || it in buttons }
        val extras = buttons.filter { it !in transport && it != ActionIds.TOGGLE_PLAY }
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceEvenly, verticalAlignment = Alignment.CenterVertically) {
            transport.forEach { NotificationButton(it, faded = false) }
            extras.forEachIndexed { i, id -> NotificationButton(id, faded = i >= 2) }
        }
    }
}

@Composable
private fun NotificationButton(id: String, faded: Boolean) {
    val scheme = MaterialTheme.colorScheme
    val play = id == ActionIds.TOGGLE_PLAY
    Box(
        Modifier.size(if (play) 48.dp else 36.dp).clip(if (play) RoundedCornerShape(14.dp) else CircleShape).background(if (play) scheme.primary else Color.Transparent),
        contentAlignment = Alignment.Center,
    ) {
        Icon(actionIdIcon(id), null, tint = (if (play) scheme.onPrimary else scheme.onSurface).copy(alpha = if (faded) 0.3f else 1f))
    }
}

/** The bottom bar as it will look: its places' icons, the first one picked, in a floating pill. */
@Composable
fun BottomBarPreview(items: List<Pair<ImageVector, String>>, modifier: Modifier = Modifier) {
    val scheme = MaterialTheme.colorScheme
    Box(modifier.fillMaxWidth().clearAndSetSemantics { }, contentAlignment = Alignment.Center) {
        Row(
            Modifier.clip(CircleShape).background(scheme.surfaceContainerHighest).padding(6.dp),
            horizontalArrangement = Arrangement.spacedBy(4.dp), verticalAlignment = Alignment.CenterVertically,
        ) {
            items.forEachIndexed { i, (icon, label) ->
                if (i == 0) {
                    Row(Modifier.clip(CircleShape).background(scheme.secondaryContainer).padding(horizontal = 14.dp, vertical = 10.dp), verticalAlignment = Alignment.CenterVertically) {
                        Icon(icon, null, tint = scheme.onSecondaryContainer)
                        Spacer(Modifier.width(8.dp))
                        Text(label, style = MaterialTheme.typography.labelLarge, color = scheme.onSecondaryContainer, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                } else {
                    Box(Modifier.padding(10.dp)) { Icon(icon, null, tint = scheme.onSurfaceVariant) }
                }
            }
        }
    }
}

/** An icon for each customisable action id (the context menu, the notification's buttons). */
fun actionIdIcon(id: String): ImageVector = when (id) {
    ActionIds.PLAY -> HocketIcons.Filled.PlayArrow
    ActionIds.PLAY_SHUFFLED, ActionIds.SHUFFLE -> HocketIcons.Filled.Shuffle
    ActionIds.PLAY_NEXT -> HocketIcons.AutoMirrored.Filled.PlaylistPlay
    ActionIds.ADD_TO_QUEUE, ActionIds.PLAY_LATER -> HocketIcons.AutoMirrored.Filled.QueueMusic
    ActionIds.PREVIEW -> HocketIcons.Filled.Headphones
    ActionIds.ADD_TO_PLAYLIST -> HocketIcons.AutoMirrored.Filled.PlaylistAdd
    ActionIds.STASH, ActionIds.CLEAR_STASH -> HocketIcons.Filled.Inventory2
    ActionIds.LOVE -> HocketIcons.Filled.Favorite
    ActionIds.UNLOVE -> HocketIcons.Filled.FavoriteBorder
    ActionIds.rate(0) -> HocketIcons.Filled.StarOutline
    ActionIds.rate(1), ActionIds.rate(2), ActionIds.rate(3), ActionIds.rate(4), ActionIds.rate(5) -> HocketIcons.Filled.Star
    ActionIds.DOWNLOAD -> HocketIcons.Filled.Download
    ActionIds.UNPIN, ActionIds.UNPIN_FROM_LIBRARY, ActionIds.UNPIN_SAVED_QUEUE -> HocketIcons.Outlined.PushPin
    ActionIds.PIN_TO_LIBRARY, ActionIds.PIN_SAVED_QUEUE -> HocketIcons.Filled.PushPin
    ActionIds.GO_TO_ALBUM -> HocketIcons.Filled.Album
    ActionIds.GO_TO_ARTIST -> HocketIcons.Filled.Person
    ActionIds.SNOOZE, ActionIds.UNSNOOZE -> HocketIcons.Filled.Snooze
    ActionIds.REMOVE_FROM_QUEUE, ActionIds.REMOVE_FROM_PLAYLIST -> HocketIcons.Filled.PlaylistRemove
    ActionIds.RESTORE_SAVED_QUEUE -> HocketIcons.Filled.History
    ActionIds.SAVE_QUEUE_AS_PLAYLIST -> HocketIcons.Filled.Save
    ActionIds.DELETE_SAVED_QUEUE, ActionIds.DELETE_PLAYLIST -> HocketIcons.Filled.Delete
    ActionIds.PREVIOUS -> HocketIcons.Filled.SkipPrevious
    ActionIds.TOGGLE_PLAY -> HocketIcons.Filled.Pause
    ActionIds.NEXT -> HocketIcons.Filled.SkipNext
    ActionIds.REPEAT -> HocketIcons.Filled.Repeat
    else -> HocketIcons.Filled.MoreVert
}
