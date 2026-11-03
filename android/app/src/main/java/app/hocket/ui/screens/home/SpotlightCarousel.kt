package app.hocket.ui.screens.home

import androidx.compose.animation.core.animateDpAsState
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.PagerState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import app.hocket.R
import app.hocket.core.ArtworkSizes
import app.hocket.ui.components.Artwork
import app.hocket.ui.icons.HocketIcons

/**
 * The Spotlight carousel: one full-bleed slide per pick, its artwork running up under the top bar
 * and fading into the page at the bottom, where its reason, title, a line about it and Play /
 * Shuffle / more sit. Each slide is drawn in a scheme from its own artwork ([schemes], one per
 * slide), always in the app's light or dark: a pale cover never turns the slide light in the dark
 * theme (unlike the immersive player, which follows the art).
 */
@Composable
fun SpotlightCarousel(
    slides: List<SpotlightSlide>,
    pager: PagerState,
    schemes: List<ColorScheme>,
    onOpen: (SpotlightSlide) -> Unit,
    onPlay: (SpotlightSlide, Boolean) -> Unit,
    onMenu: (SpotlightSlide) -> Unit,
    modifier: Modifier = Modifier,
) {
    BoxWithConstraints(modifier.fillMaxWidth().testTag("home.spotlight")) {
        val height = (maxWidth * 1.18f).coerceIn(440.dp, 600.dp)
        HorizontalPager(pager, Modifier.fillMaxWidth().height(height), key = { slides.getOrNull(it)?.key ?: it }) { page ->
            val slide = slides.getOrNull(page) ?: return@HorizontalPager
            MaterialTheme(colorScheme = schemes.getOrNull(page) ?: MaterialTheme.colorScheme) {
                SpotlightPage(slide, page, slides.size, onOpen, onPlay, onMenu)
            }
        }
        if (slides.size > 1) {
            // Outside the pager, so the dots stay put while slides move; level with the buttons.
            Row(
                Modifier.align(Alignment.BottomEnd).padding(end = 24.dp, bottom = 24.dp).height(56.dp).clearAndSetSemantics { },
                horizontalArrangement = Arrangement.spacedBy(6.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                slides.indices.forEach { i ->
                    val on = i == pager.currentPage
                    val w by animateDpAsState(if (on) 22.dp else 8.dp, label = "dot")
                    Box(Modifier.size(width = w, height = 8.dp).background(if (on) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface.copy(alpha = 0.35f), CircleShape))
                }
            }
        }
    }
}

@Composable
private fun SpotlightPage(slide: SpotlightSlide, page: Int, count: Int, onOpen: (SpotlightSlide) -> Unit, onPlay: (SpotlightSlide, Boolean) -> Unit, onMenu: (SpotlightSlide) -> Unit) {
    val cs = MaterialTheme.colorScheme
    val eyebrow = eyebrowOf(slide)
    val subtitle = subtitleOf(slide)
    val position = stringResource(R.string.spotlight_page, page + 1, count)
    val a11y = stringResource(R.string.spotlight_a11y, eyebrow, slide.title, subtitle) + ". " + position
    Box(Modifier.fillMaxSize().background(cs.surface).testTag("home.spotlight.${slide.kind.name}")) {
        Artwork(slide.coverArt, ArtworkSizes.HERO, null, Modifier.fillMaxSize(), RectangleShape)
        // A wash at the top for the bar's lockup and buttons, and the page's own surface rising
        // under the text so it reads on any cover.
        // A pale cover stays a pale picture, but never lights the slide: the wash is the theme's surface.
        Box(Modifier.fillMaxSize().background(Brush.verticalGradient(
            0f to cs.surface.copy(alpha = 0.9f), 0.14f to cs.surface.copy(alpha = 0.62f), 0.3f to cs.surface.copy(alpha = 0.12f),
            0.42f to cs.surface.copy(alpha = 0.12f), 0.62f to cs.surface.copy(alpha = 0.8f), 0.8f to cs.surface.copy(alpha = 0.95f), 1f to cs.surface,
        )))
        Column(Modifier.align(Alignment.BottomStart).fillMaxWidth().padding(start = 24.dp, end = 24.dp, bottom = 24.dp)) {
            // The words open the album, artist or playlist (not the whole slide: its top is under the bar's buttons).
            Column(Modifier.fillMaxWidth().clickable { onOpen(slide) }.clearAndSetSemantics { contentDescription = a11y; role = Role.Button }.testTag("home.spotlight.open")) {
                Text(
                    eyebrow.uppercase(),
                    style = MaterialTheme.typography.labelLarge,
                    fontWeight = FontWeight.SemiBold,
                    letterSpacing = 1.2.sp,
                    color = cs.onSurface,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.background(cs.onSurface.copy(alpha = 0.12f), CircleShape).padding(horizontal = 14.dp, vertical = 6.dp),
                )
                Spacer(Modifier.height(12.dp))
                Text(slide.title, style = MaterialTheme.typography.displaySmall, fontWeight = FontWeight.Bold, color = cs.onSurface, maxLines = 2, overflow = TextOverflow.Ellipsis)
                Spacer(Modifier.height(4.dp))
                Text(subtitle, style = MaterialTheme.typography.bodyLarge, color = cs.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            Spacer(Modifier.height(20.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                val resume = slide.resume != null
                Button(onClick = { onPlay(slide, false) }, modifier = Modifier.height(56.dp).testTag("home.spotlight.play"), contentPadding = PaddingValues(start = 20.dp, end = 26.dp)) {
                    Icon(HocketIcons.Filled.PlayArrow, null, Modifier.size(24.dp))
                    Spacer(Modifier.width(8.dp))
                    Text(stringResource(if (resume) R.string.action_resume else R.string.action_play), style = MaterialTheme.typography.titleMedium)
                }
                Spacer(Modifier.width(12.dp))
                FilledTonalIconButton(onClick = { onPlay(slide, true) }, modifier = Modifier.size(56.dp), shape = RoundedCornerShape(18.dp)) {
                    Icon(HocketIcons.Filled.Shuffle, stringResource(R.string.action_shuffle))
                }
                Spacer(Modifier.width(4.dp))
                IconButton(onClick = { onMenu(slide) }) { Icon(HocketIcons.Filled.MoreVert, stringResource(R.string.action_more)) }
            }
        }
    }
}

@Composable
private fun eyebrowOf(slide: SpotlightSlide): String = when {
    slide.kind == SlideKind.TopGenre && slide.genre != null -> stringResource(R.string.spotlight_top_genre, slide.genre)
    slide.kind == SlideKind.TopGenre -> stringResource(R.string.spotlight_top_genre_plain)
    else -> stringResource(slide.kind.eyebrow)
}

/** "Halden Fields · 2025 · 23 plays"; "… · Track 4 of 10" to resume; an artist's albums; a playlist's songs. */
@Composable
private fun subtitleOf(slide: SpotlightSlide): String {
    slide.artist?.let { return stringResource(R.string.library_count_albums, it.albumCount.toInt()) }
    slide.playlist?.let { p -> return listOfNotNull(p.owner?.takeIf { !p.isMine }, stringResource(R.string.library_count_songs, p.songCount.toInt())).joinToString(" · ") }
    val a = slide.album ?: return ""
    val artist = a.artist ?: stringResource(R.string.unknown_artist)
    val resume = slide.resume
    val third = when {
        resume != null -> stringResource(R.string.spotlight_resume_track, resume.trackIndex.toInt() + 1, a.songCount.toInt())
        a.playCount > 0u && slide.kind != SlideKind.NewlyAdded && slide.kind != SlideKind.LuckyDip -> pluralStringResource(R.plurals.album_plays, a.playCount.toInt(), a.playCount.toInt())
        else -> null
    }
    return listOfNotNull(artist, a.year?.takeIf { it > 0u }?.toString(), third).joinToString(" · ")
}
