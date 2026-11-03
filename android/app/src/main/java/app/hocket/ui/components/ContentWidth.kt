package app.hocket.ui.components

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.wrapContentWidth
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/*
 * Sizes for content areas wider than a phone (a tablet, a landscape window). Every decision here is
 * on the content area's own width (a BoxWithConstraints around the screen or list), never the
 * window's: the shell may keep a side panel beside it.
 */

/** The widest a list's rows grow: past this, lines of text get too long to scan. */
val ContentMaxWidth = 840.dp

/** Below this content width a screen is laid out as on a phone, and stays exactly as it was. */
val PhoneMaxWidth = 600.dp

/** From this content width, list-detail screens (the library lists, settings) show both panes. */
val TwoPaneMinWidth = 840.dp

/** The list pane's width in a two-pane screen: a phone's list, the rest goes to the detail. */
val ListPaneWidth = 380.dp

/** Centres this content, at most [ContentMaxWidth] wide, in the width it is given. */
fun Modifier.contentMaxWidth(): Modifier =
    fillMaxWidth().wrapContentWidth(Alignment.CenterHorizontally).widthIn(max = ContentMaxWidth).fillMaxWidth()

/**
 * The side padding that centres a list's content at [ContentMaxWidth] in [width]: for a lazy
 * list's contentPadding, so the list itself (its scrolling, its edge fades) still spans the screen.
 */
fun contentSideInset(width: Dp): Dp = ((width - ContentMaxWidth) / 2).coerceAtLeast(0.dp)

/**
 * Columns for a grid of [minCell]-wide cells across [width] with [gap] between them: exactly two on
 * a phone (so phone layouts never change), else as many as fit, at least two and at most [max].
 */
fun adaptiveColumns(width: Dp, minCell: Dp, gap: Dp = 0.dp, max: Int = Int.MAX_VALUE): Int {
    if (width < PhoneMaxWidth) return 2
    val fit = ((width + gap) / (minCell + gap)).toInt()
    return fit.coerceIn(2, max.coerceAtLeast(2))
}

/**
 * Content padding for a full-width lazy list in a content area [width] wide: [top], [bottom], and
 * sides that keep its rows at [ContentMaxWidth] ([horizontal] on top of those).
 */
fun maxWidthListPadding(width: Dp, top: Dp = 0.dp, bottom: Dp = 0.dp, horizontal: Dp = 0.dp): androidx.compose.foundation.layout.PaddingValues {
    val side = contentSideInset(width) + horizontal
    return androidx.compose.foundation.layout.PaddingValues(start = side, end = side, top = top, bottom = bottom)
}
