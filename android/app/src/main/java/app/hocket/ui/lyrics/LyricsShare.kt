package app.hocket.ui.lyrics

import android.graphics.Bitmap
import android.view.View
import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.snap
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.foundation.layout.requiredWidth
import androidx.compose.foundation.layout.wrapContentSize
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.graphics.GraphicsLayerScope
import androidx.compose.ui.graphics.TransformOrigin
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.layer.GraphicsLayer
import androidx.compose.ui.graphics.layer.drawLayer
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.graphics.rememberGraphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.LayoutCoordinates
import androidx.compose.ui.layout.boundsInWindow
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.colorResource
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.util.lerp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.compose.ui.window.DialogWindowProvider
import app.hocket.R
import app.hocket.core.ArtworkSizes
import app.hocket.core.api.LyricLine
import app.hocket.core.api.Lyrics
import app.hocket.core.api.TrackSummary
import app.hocket.share.ShareImages
import app.hocket.ui.a11y.LocalReducedMotion
import app.hocket.ui.brand.Lockup
import app.hocket.ui.components.Artwork
import app.hocket.ui.components.ChoiceRow
import app.hocket.ui.icons.HocketIcons
import app.hocket.ui.player.artworkSeed
import app.hocket.ui.theme.Motion
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch

/** Lines a card can hold: past this it stops reading as a quote. */
internal const val MAX_SHARED_LINES = 6

/**
 * A held lyric line as the list draws it: its [bounds] on screen (with the list's scaling of the
 * line), the width its text is laid out in, and its text style and colour.
 */
@Immutable
internal class SourceLine(val bounds: Rect, val layoutWidth: Int, val style: TextStyle, val color: Color)

/**
 * A long-press on lyric [line]. [source] says where that line sits on screen now (null once it has
 * scrolled away): its text flies from there into the card, and back on close.
 */
@Immutable
internal class LyricsShareRequest(val line: Int, val source: () -> SourceLine?)

/** Where the held line's text sits in the card (relative to the card) and how the card draws it. */
@Immutable
private class TrackedText(val offset: Offset, val width: Int, val style: TextStyle, val color: Color)

internal enum class CardBackground { Artwork, Colour }
internal enum class CardAlign { Start, Centre }
internal enum class CardShape(val ratio: Float) { Square(1f), Tall(9f / 16f) }

/** How the card looks; the share screen's options. */
@Immutable
internal data class LyricsCardStyle(
    val background: CardBackground = CardBackground.Artwork,
    val align: CardAlign = CardAlign.Start,
    val shape: CardShape = CardShape.Square,
)

/** Where [coords] sit on screen (window bounds plus the window's origin), or null once detached. */
internal fun screenBounds(view: View, coords: LayoutCoordinates?): Rect? {
    if (coords == null || !coords.isAttached) return null
    val origin = IntArray(2).also { view.rootView.getLocationOnScreen(it) }
    return coords.boundsInWindow().translate(origin[0].toFloat(), origin[1].toFloat())
}

/**
 * Toggles lyric [index] in [selected]: never below one line, never past [MAX_SHARED_LINES]. A line
 * that would go past the cap is ignored.
 */
internal fun toggleSharedLine(selected: Set<Int>, index: Int): Set<Int> = when {
    index in selected -> if (selected.size > 1) selected - index else selected
    selected.size < MAX_SHARED_LINES -> selected + index
    else -> selected
}

/** The exported card's width in pixels, whatever its size on screen. */
private const val EXPORT_WIDTH_PX = 1080f

/**
 * The lyrics share screen, from a long-press on a lyric line: a card (album art, title and artist at
 * the top, the chosen lines, the app's icon and name at the bottom, over the player's blurred
 * artwork) with the options (which lines, background, alignment, shape) and Save / Share below it.
 *
 * The held line's text flies out of the lyrics into its place on the card, moving and resizing from
 * the list's type to the card's (two copies crossfading on the way, each laid out as its end draws
 * it), while the card forms around it from the line and the screen behind dims. Meanwhile the line
 * in the list and its copy on the card are hidden, so neither end jumps. Closing runs it all back
 * into wherever the line is now. With reduced motion the card simply appears.
 */
@Composable
internal fun LyricsShareDialog(doc: Lyrics, track: TrackSummary, request: LyricsShareRequest, onDismiss: () -> Unit) {
    val reducedMotion = LocalReducedMotion.current
    val scope = rememberCoroutineScope()
    val context = LocalContext.current
    val progress = remember { Animatable(0f) }
    // Where the card grows from and shrinks back to: the held line, re-read on close (the lyrics
    // keep scrolling under the dialog while the song plays).
    var from by remember { mutableStateOf(request.source()) }
    var closing by remember { mutableStateOf(false) }
    val close: () -> Unit = {
        if (!closing) {
            closing = true
            request.source()?.let { from = it }
            scope.launch { progress.animateTo(0f, if (reducedMotion) snap() else Motion.mode); onDismiss() }
        }
    }
    var selected by remember(request.line) { mutableStateOf(setOf(request.line)) }
    var style by remember { mutableStateOf(LyricsCardStyle()) }
    var target by remember { mutableStateOf<Rect?>(null) }
    var tracked by remember { mutableStateOf<TrackedText?>(null) }
    var overlayOrigin by remember { mutableStateOf(Offset.Zero) }
    var busy by remember { mutableStateOf(false) }
    // The card's recording; created in the dialog's window, which draws it.
    val layerRef = remember { arrayOfNulls<GraphicsLayer>(1) }
    val name = remember(track.title) { ShareImages.fileName(track.title) }
    val failed = stringResource(R.string.lyrics_share_failed)
    val saved = stringResource(R.string.lyrics_share_saved)
    val savedDocument = stringResource(R.string.lyrics_share_saved_document)
    val chooserTitle = stringResource(R.string.lyrics_share)
    fun toast(text: String) = Toast.makeText(context, text, Toast.LENGTH_SHORT).show()

    // Before Android 10 the image goes to a document the user picks (MediaStore needs a permission).
    var pending by remember { mutableStateOf<Bitmap?>(null) }
    val createDocument = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("image/png")) { uri ->
        val bitmap = pending
        pending = null
        if (uri != null && bitmap != null) scope.launch { toast(if (ShareImages.writeTo(context, uri, bitmap)) savedDocument else failed) }
    }

    fun withImage(block: suspend (Bitmap) -> Unit) {
        val layer = layerRef[0]
        if (busy || layer == null) return
        busy = true
        scope.launch {
            try {
                block(layer.toImageBitmap().asAndroidBitmap())
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                // No image, no room to write it, or no app to take it.
                toast(failed)
            } finally { busy = false }
        }
    }

    Dialog(onDismissRequest = close, properties = DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false)) {
        // The window's own dim would snap in; the scrim below fades with the card instead.
        val dialogView = LocalView.current
        SideEffect { (dialogView.parent as? DialogWindowProvider)?.window?.setDimAmount(0f) }
        val layer = rememberGraphicsLayer()
        SideEffect { layerRef[0] = layer }
        // Grow once the card's place is known, so the first frame starts at the line.
        LaunchedEffect(target != null) {
            if (target != null && !closing) progress.animateTo(1f, if (reducedMotion) snap() else Motion.mode)
        }
        // The progress is read in the layers only: the motion redraws, it does not recompose.
        val shown = Modifier.graphicsLayer { alpha = progress.value.coerceIn(0f, 1f) }
        // Settled open: the card shows its own copy of the line, and nothing flies.
        val settled = { !closing && !progress.isRunning && target != null }
        Box(Modifier.fillMaxSize().testTag("lyrics.share")) {
            Box(
                Modifier.fillMaxSize()
                    .then(shown)
                    .background(Color.Black.copy(alpha = 0.72f))
                    // A tap outside closes; not an accessibility node (Close and back do the same).
                    .pointerInput(Unit) { detectTapGestures { close() } },
            )
            BoxWithConstraints(Modifier.fillMaxSize().statusBarsPadding()) {
                val optionsMax = maxHeight * 0.6f
                Column(Modifier.fillMaxSize()) {
                    Row(Modifier.fillMaxWidth().padding(horizontal = 4.dp).then(shown).consumeTaps(), verticalAlignment = Alignment.CenterVertically) {
                        IconButton(onClick = close, modifier = Modifier.testTag("lyrics.share.close")) {
                            Icon(HocketIcons.Filled.Close, stringResource(R.string.lyrics_share_close), tint = Color.White)
                        }
                        Text(stringResource(R.string.lyrics_share), style = MaterialTheme.typography.titleLarge, color = Color.White, modifier = Modifier.semantics { heading() })
                    }
                    val ratio by animateFloatAsState(style.shape.ratio, if (reducedMotion) snap() else Motion.mode, label = "cardRatio")
                    Box(Modifier.weight(1f).fillMaxWidth().padding(horizontal = 32.dp, vertical = 16.dp), contentAlignment = Alignment.Center) {
                        val view = LocalView.current
                        val lines = remember(doc, selected) { doc.lines.filterIndexed { i, _ -> i in selected } }
                        // The held line's place among the card's lines (they keep the lyrics' order).
                        val heldPosition = heldAt(selected, request.line)
                        Box(Modifier.aspectRatio(ratio).onGloballyPositioned { target = screenBounds(view, it) }.consumeTaps()) {
                            LyricsCard(
                                track, lines, style, layer,
                                tracked = heldPosition,
                                trackedAlpha = { if (settled()) 1f else 0f },
                                onTracked = { offset, width, textStyle, color -> tracked = TrackedText(offset, width, textStyle, color) },
                                modifier = Modifier.fillMaxSize().graphicsLayer {
                                    val p = progress.value
                                    val start = from?.bounds
                                    val to = target
                                    // From the held line (its centre, as tall as the line) to the card's place.
                                    if (start != null && to != null && to.width > 0f && to.height > 0f) {
                                        val s = lerp(minOf(start.width / to.width, start.height / to.height).coerceIn(0.1f, 1f), 1f, p)
                                        scaleX = s; scaleY = s
                                        translationX = lerp(start.center.x - to.center.x, 0f, p)
                                        translationY = lerp(start.center.y - to.center.y, 0f, p)
                                    }
                                    // Forms around the text as it flies in, a little behind it.
                                    alpha = (p * 1.6f).coerceIn(0f, 1f)
                                },
                            )
                        }
                    }
                    ShareOptions(
                        doc, selected, style,
                        onToggle = { selected = toggleSharedLine(selected, it) },
                        onStyle = { change -> style = change(style) },
                        busy = busy,
                        onSave = {
                            withImage { bitmap ->
                                if (ShareImages.canSaveToPictures) {
                                    toast(if (ShareImages.saveToPictures(context, bitmap, name)) saved else failed)
                                } else {
                                    pending = bitmap
                                    createDocument.launch(name)
                                }
                            }
                        },
                        onShare = { withImage { ShareImages.share(context, it, name, chooserTitle) } },
                        initialLine = request.line,
                        modifier = Modifier.heightIn(max = optionsMax).graphicsLayer {
                            // Slides up from below as the card grows.
                            val p = progress.value
                            translationY = (1f - p) * size.height
                            alpha = p.coerceIn(0f, 1f)
                        },
                    )
                }
            }
            // The held line in flight, over everything (and out of the accessibility tree: the
            // card already speaks its lines).
            val view = LocalView.current
            Box(Modifier.fillMaxSize().onGloballyPositioned { c -> screenBounds(view, c)?.let { overlayOrigin = it.topLeft } }.clearAndSetSemantics { }) {
                val start = from
                val end = tracked
                val text = doc.lines.getOrNull(request.line)?.text
                if (start != null && end != null && text != null && heldAt(selected, request.line) >= 0) {
                    // Both copies follow one path: the top-left corner and the type size move from
                    // the list's to the card's; each is scaled from its own type size to that.
                    fun GraphicsLayerScope.fly(ownSize: Float, visible: (Float) -> Float) {
                        val card = target
                        val p = progress.value
                        if (card == null || settled()) { alpha = 0f; return }
                        val from0 = start.bounds.topLeft - overlayOrigin
                        val to0 = card.topLeft + end.offset - overlayOrigin
                        val sourceSize = start.style.fontSize.toPx() * (start.bounds.width / start.layoutWidth.coerceAtLeast(1))
                        val typeSize = lerp(sourceSize, end.style.fontSize.toPx(), p)
                        transformOrigin = TransformOrigin(0f, 0f)
                        translationX = lerp(from0.x, to0.x, p)
                        translationY = lerp(from0.y, to0.y, p)
                        scaleX = typeSize / ownSize; scaleY = scaleX
                        alpha = visible(((p - 0.2f) / 0.5f).coerceIn(0f, 1f))
                    }
                    FlyingText(text, start.style, start.color, start.layoutWidth) { fly(start.style.fontSize.toPx()) { 1f - it } }
                    FlyingText(text, end.style, end.color, end.width) { fly(end.style.fontSize.toPx()) { it } }
                }
            }
        }
    }
}

/** Takes taps without doing anything, so they do not reach the scrim (which closes). */
private fun Modifier.consumeTaps(): Modifier = pointerInput(Unit) { detectTapGestures { } }

/** [line]'s place among the [selected] lines on the card (they keep the lyrics' order), or -1. */
private fun heldAt(selected: Set<Int>, line: Int): Int = if (line in selected) selected.count { it < line } else -1

/** A copy of the held line laid out [widthPx] wide at the top-left corner, moved by [layer]. */
@Composable
private fun FlyingText(text: String, style: TextStyle, color: Color, widthPx: Int, layer: GraphicsLayerScope.() -> Unit) {
    val width = with(LocalDensity.current) { widthPx.toDp() }
    Text(text, style = style, color = color, modifier = Modifier.wrapContentSize(Alignment.TopStart, unbounded = true).graphicsLayer(layer).requiredWidth(width))
}

/**
 * The shareable card. Every size is relative to the card's width (a 320 dp reference, 250 dp when
 * tall), so the exported image looks the same at any card size and ignores the font scale. Also
 * recorded into [layer] at [EXPORT_WIDTH_PX] wide (and drawn from it, scaled down): what Save and
 * Share export.
 */
@Composable
internal fun LyricsCard(
    track: TrackSummary,
    lines: List<LyricLine>,
    style: LyricsCardStyle,
    layer: GraphicsLayer?,
    modifier: Modifier = Modifier,
    tracked: Int = -1,
    trackedAlpha: () -> Float = { 1f },
    onTracked: (offset: Offset, width: Int, style: TextStyle, color: Color) -> Unit = { _, _, _, _ -> },
) {
    // Where line [tracked] is laid out in the card, reported whenever the card or it moves.
    val placed = remember { arrayOfNulls<LayoutCoordinates>(2) }
    val trackedLook = remember { arrayOfNulls<Any>(2) }
    fun report() {
        val root = placed[0]; val text = placed[1]
        if (root == null || text == null || !root.isAttached || !text.isAttached) return
        onTracked(root.localPositionOf(text, Offset.Zero), text.size.width, trackedLook[0] as TextStyle, trackedLook[1] as Color)
    }
    val cardLabel = stringResource(R.string.lyrics_share_card_a11y, lines.joinToString(" ") { it.text })
    BoxWithConstraints(
        modifier
            .let { m ->
                if (layer == null) m else m.drawWithContent {
                    val k = (EXPORT_WIDTH_PX / size.width).coerceAtLeast(1f)
                    layer.record(size = IntSize(kotlin.math.ceil(size.width * k).toInt(), kotlin.math.ceil(size.height * k).toInt())) {
                        scale(k, k, pivot = Offset.Zero) { this@drawWithContent.drawContent() }
                    }
                    scale(1f / k, 1f / k, pivot = Offset.Zero) { drawLayer(layer) }
                }
            }
            .onGloballyPositioned { placed[0] = it; report() }
            .clip(RoundedCornerShape(percent = 7))
            .testTag("lyrics.share.card")
            // One node: the lines once, not the description and then each line again.
            .clearAndSetSemantics { contentDescription = cardLabel },
    ) {
        // The tall card is narrower for its height: sized against a narrower reference, so its
        // content is not lost in the space (the lines still shrink to fit).
        val unit = maxWidth / if (style.shape == CardShape.Tall) 250.dp else 320.dp
        val density = LocalDensity.current
        fun textSize(dp: Dp) = with(density) { (dp * unit).toSp() }
        fun d(dp: Dp) = dp * unit

        when (style.background) {
            CardBackground.Artwork -> {
                Box(Modifier.matchParentSize().background(Color.Black))
                LyricsBackground(track.coverArt, animated = false, visible = false, modifier = Modifier.matchParentSize(), tag = "lyrics.share.background")
            }
            CardBackground.Colour -> {
                val seed = artworkSeed(track.coverArt).value ?: Color.DarkGray
                Box(Modifier.matchParentSize().background(Brush.verticalGradient(0f to seed, 1f to lerp(seed, Color.Black, 0.55f))))
                // The lyrics scrim's rule: enough black for white text over the brightest part (the top).
                val scrim = remember(seed) { LyricsContrast.scrimAlpha(seed) }
                Box(Modifier.matchParentSize().background(Color.Black.copy(alpha = scrim)))
            }
        }

        val centred = style.align == CardAlign.Centre
        val textAlign = if (centred) TextAlign.Center else TextAlign.Start
        // The lines at the largest size (up to 1x) that fits between the header and the footer.
        val measurer = rememberTextMeasurer()
        val widthPx = constraints.maxWidth - with(density) { d(48.dp).roundToPx() }
        val roomPx = constraints.maxHeight - with(density) { d(48.dp + 52.dp + 22.dp + 32.dp).roundToPx() }
        fun lineStyle(line: LyricLine, fit: Float): TextStyle {
            val base = if (line.background) 18.dp else 26.dp
            return TextStyle(fontSize = textSize(base * fit), lineHeight = textSize(base * fit * 1.2f), fontWeight = if (line.background) FontWeight.Medium else FontWeight.Bold, textAlign = textAlign)
        }
        val fit = remember(lines, widthPx, roomPx, textAlign, unit) {
            if (widthPx <= 0 || roomPx <= 0) return@remember 0.4f
            val gap = with(density) { d(8.dp).roundToPx() }
            var f = 1f
            while (f > 0.4f) {
                val height = lines.sumOf { measurer.measure(it.text, lineStyle(it, f), constraints = Constraints(maxWidth = widthPx)).size.height } + gap * (lines.size - 1)
                if (height <= roomPx) break
                f -= 0.05f
            }
            f
        }
        Column(Modifier.matchParentSize().padding(d(24.dp)), horizontalAlignment = if (centred) Alignment.CenterHorizontally else Alignment.Start) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Artwork(track.coverArt, ArtworkSizes.GRID, null, Modifier.size(d(52.dp)), RoundedCornerShape(d(10.dp)))
                Spacer(Modifier.width(d(12.dp)))
                Column(Modifier.weight(1f, fill = false)) {
                    Text(track.title, style = TextStyle(fontSize = textSize(17.dp), fontWeight = FontWeight.Bold, lineHeight = textSize(21.dp)), color = Color.White, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    track.artist?.let { Text(it, style = TextStyle(fontSize = textSize(14.dp), lineHeight = textSize(18.dp)), color = LyricsContrast.secondary, maxLines = 1, overflow = TextOverflow.Ellipsis) }
                }
            }
            Column(
                Modifier.weight(1f).fillMaxWidth().padding(vertical = d(16.dp)),
                verticalArrangement = Arrangement.spacedBy(d(8.dp), Alignment.CenterVertically),
                horizontalAlignment = if (centred) Alignment.CenterHorizontally else Alignment.Start,
            ) {
                lines.forEachIndexed { i, line ->
                    val lineStyle = lineStyle(line, fit)
                    val color = if (line.background) LyricsContrast.secondary else Color.White
                    val modifier = if (i != tracked) Modifier.fillMaxWidth() else {
                        trackedLook[0] = lineStyle; trackedLook[1] = color
                        Modifier.fillMaxWidth().onGloballyPositioned { placed[1] = it; report() }.graphicsLayer { alpha = trackedAlpha() }
                    }
                    Text(line.text, style = lineStyle, color = color, modifier = modifier)
                }
            }
            // The brand's lockup in white (docs/brand.md): the bare wave and the word mark.
            Lockup(fontSize = d(15.dp))
        }
    }
}


/** What goes on the card and the Save / Share buttons. */
@Composable
private fun ShareOptions(
    doc: Lyrics,
    selected: Set<Int>,
    style: LyricsCardStyle,
    onToggle: (Int) -> Unit,
    onStyle: ((LyricsCardStyle) -> LyricsCardStyle) -> Unit,
    busy: Boolean,
    onSave: () -> Unit,
    onShare: () -> Unit,
    initialLine: Int,
    modifier: Modifier = Modifier,
) {
    Surface(modifier.fillMaxWidth(), shape = RoundedCornerShape(topStart = 28.dp, topEnd = 28.dp), color = MaterialTheme.colorScheme.surfaceContainerHigh) {
        Column(Modifier.verticalScroll(rememberScrollState()).navigationBarsPadding().padding(top = 16.dp, bottom = 12.dp)) {
            Text(stringResource(R.string.lyrics_share_lines, MAX_SHARED_LINES), style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(horizontal = 24.dp))
            val pickable = remember(doc) { doc.lines.withIndex().filter { it.value.text.isNotBlank() } }
            val start = remember(doc) { (pickable.indexOfFirst { it.index == initialLine } - 1).coerceAtLeast(0) }
            val listState = rememberLazyListState(initialFirstVisibleItemIndex = start)
            LazyColumn(Modifier.fillMaxWidth().heightIn(max = 168.dp).testTag("lyrics.share.lines"), state = listState) {
                items(pickable, key = { it.index }) { (i, line) ->
                    val on = i in selected
                    Row(
                        Modifier.fillMaxWidth().heightIn(min = 48.dp)
                            .toggleable(value = on, role = Role.Checkbox, onValueChange = { onToggle(i) })
                            .padding(horizontal = 24.dp, vertical = 8.dp)
                            .testTag("lyrics.share.line.$i"),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Box(Modifier.size(24.dp), contentAlignment = Alignment.Center) {
                            if (on) Icon(HocketIcons.Filled.Check, null, tint = MaterialTheme.colorScheme.primary)
                        }
                        Spacer(Modifier.width(12.dp))
                        Text(line.text, style = MaterialTheme.typography.bodyLarge, color = if (on) MaterialTheme.colorScheme.onSurface else MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 2, overflow = TextOverflow.Ellipsis)
                    }
                }
            }
            Column(Modifier.padding(horizontal = 24.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                OptionRow(stringResource(R.string.lyrics_share_background)) {
                    ChoiceRow(
                        listOf(CardBackground.Artwork to stringResource(R.string.lyrics_share_background_artwork), CardBackground.Colour to stringResource(R.string.lyrics_share_background_colour)),
                        isSelected = { it == style.background }, onSelect = { v -> onStyle { it.copy(background = v) } },
                    )
                }
                OptionRow(stringResource(R.string.lyrics_share_align)) {
                    ChoiceRow(
                        listOf(CardAlign.Start to stringResource(R.string.lyrics_share_align_start), CardAlign.Centre to stringResource(R.string.lyrics_share_align_centre)),
                        isSelected = { it == style.align }, onSelect = { v -> onStyle { it.copy(align = v) } },
                    )
                }
                OptionRow(stringResource(R.string.lyrics_share_shape)) {
                    ChoiceRow(
                        listOf(CardShape.Square to stringResource(R.string.lyrics_share_shape_square), CardShape.Tall to stringResource(R.string.lyrics_share_shape_tall)),
                        isSelected = { it == style.shape }, onSelect = { v -> onStyle { it.copy(shape = v) } },
                    )
                }
            }
            Row(Modifier.fillMaxWidth().padding(horizontal = 24.dp, vertical = 8.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                FilledTonalButton(onClick = onSave, enabled = !busy, modifier = Modifier.weight(1f).heightIn(min = 48.dp).testTag("lyrics.share.save")) {
                    Icon(HocketIcons.Filled.Download, null)
                    Spacer(Modifier.width(8.dp))
                    Text(stringResource(R.string.lyrics_share_save))
                }
                Button(onClick = onShare, enabled = !busy, modifier = Modifier.weight(1f).heightIn(min = 48.dp).testTag("lyrics.share.send")) {
                    Icon(HocketIcons.Filled.Share, null)
                    Spacer(Modifier.width(8.dp))
                    Text(stringResource(R.string.lyrics_share_send))
                }
            }
        }
    }
}

/** A labelled option: the label above its choices. */
@Composable
private fun OptionRow(label: String, content: @Composable () -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Text(label, style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
        content()
    }
}
