package app.hocket.ui

import androidx.activity.ComponentActivity
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.click
import androidx.compose.ui.test.swipe
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.performTouchInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.Commands
import app.hocket.core.SettingKeys
import app.hocket.ui.nav.AppRoot
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * The full player's page (#40): the artwork right under the handle with the title right under it
 * and the rest spread over the remaining height; the album beside the thumbnail in the other
 * modes; the queue's name opening the queue and its chevron the queue switcher; "Playing on" and the
 * accent device button only while another device plays; the title opening the song's details; the
 * draggable stars and the setting that hides them; and the skip swipe's peeking carousel.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class FullPlayerLayoutTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val core = TestCore()

    private fun exists(tag: String) = compose.onAllNodesWithTag(tag).fetchSemanticsNodes().isNotEmpty()
    private fun displayed(tag: String) = runCatching { compose.onNodeWithTag(tag).assertIsDisplayed() }.isSuccess
    private fun bounds(tag: String): Rect = compose.onNodeWithTag(tag).fetchSemanticsNode().boundsInRoot
    /**
     * A semantics click, then the state it wrote handed to composition: with the clock under manual
     * control nothing else sends the global snapshot's apply notifications (a device's looper would).
     */
    private fun tap(tag: String) {
        compose.onNodeWithTag(tag).performSemanticsAction(SemanticsActions.OnClick)
        compose.runOnUiThread { androidx.compose.runtime.snapshots.Snapshot.sendApplyNotifications() }
    }
    private val dp get() = compose.activity.resources.displayMetrics.density

    private fun startExpanded() {
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        compose.waitUntil(5_000) { exists("miniPlayer") }
        compose.onNodeWithTag("nowPlaying.sheet").performSemanticsAction(SemanticsActions.Expand)
        compose.waitUntil(5_000) { exists("player.playPause") }
        compose.waitForIdle()
    }

    @Test
    fun theArtworkIsAtTheTopWithTheTitleUnderItAndTheRestSpreadOut() {
        startExpanded()
        val handle = bounds("player.collapse")
        val art = bounds("player.artwork")
        val title = bounds("player.title")
        val play = bounds("player.playPause")
        val actions = bounds("player.mode.queue")
        assertTrue("artwork right under the handle: $handle / $art", art.top - handle.bottom in 0f..24f * dp)
        assertTrue("title right under the artwork: $art / $title", title.top - art.bottom in 0f..40f * dp)
        // The spare height goes between the controls (and under the last), not into one band:
        // the transport and the action row are well apart but nothing hangs off the bottom.
        val root = compose.onNodeWithTag("nowPlaying.sheet").fetchSemanticsNode().boundsInRoot
        val gap = actions.top - play.bottom
        assertTrue("the rest spread out (gap $gap)", gap > 16f * dp)
        assertTrue("the action row clear of the screen's bottom (${actions.bottom} in $root)", actions.bottom < root.bottom)
        assertTrue("and not all pushed to the bottom", root.bottom - actions.bottom > 12f * dp)
        // The action row: queue name and its chevron, the device switcher, lyrics, more, in a line.
        val row = listOf("player.mode.queue", "player.switchQueue", "player.connect", "player.mode.lyrics", "player.more").map(::bounds)
        row.zipWithNext().forEach { (a, b) -> assertTrue("in order: $a then $b", b.left >= a.right - 1f) }
        row.forEach { assertTrue("48 dp tall: $it", it.height >= 48f * dp - 1f) }
    }

    /** The seek bar's wave (6 dp into its touch area) and the star glyphs' bottoms (17.5 dp into their 20 dp icons). */
    private fun waveTop() = bounds("player.seekBar").top + 6f * dp
    private fun starsBottom() = bounds("rating").top + 17.5f * dp

    /** The elapsed time's baseline (the times are hidden from accessibility, read by the seek bar). */
    private fun timesBaseline(): Float {
        val node = compose.onAllNodesWithTag("player.elapsed", useUnmergedTree = true).fetchSemanticsNodes().single()
        val results = mutableListOf<androidx.compose.ui.text.TextLayoutResult>()
        node.config[SemanticsActions.GetTextLayoutResult].action!!.invoke(results)
        return node.positionInRoot.y + results.first().firstBaseline
    }

    /** The times-to-transport gap, and the title block's (stars, or the artist line) to the wave. */
    private fun gaps(stars: Boolean): Pair<Float, Float> {
        val track = core.client.nowPlaying.value!!.track
        val bottom = if (stars) starsBottom() else baseline(track.artist!!)
        return (bounds("player.playPause").top - timesBaseline()) to (waveTop() - bottom)
    }

    private fun showMode(tag: String) {
        compose.mainClock.autoAdvance = false
        tap(tag)
        compose.mainClock.advanceTimeBy(1_500)
    }

    /**
     * The title block is as far above the seek bar's wave as the times are above the transport, in
     * every mode, with or without the stars; the stars' touch row stops short of the seek bar's;
     * the action row is as far above the bottom as above the transport; and the artwork is centred
     * in what is left above the controls.
     */
    private fun assertControlsLayout() {
        val root = compose.onNodeWithTag("nowPlaying.sheet").fetchSemanticsNode().boundsInRoot
        val handle = bounds("player.collapse")
        val art = bounds("player.artwork")
        val title = bounds("player.title")
        val play = bounds("player.playPause")
        val actions = bounds("player.mode.queue")
        assertEquals("the artwork as wide as the margins allow", 371f * dp, art.width, 1f)
        assertEquals("the artwork centred above the controls", art.top - handle.bottom - 8f * dp, title.top - art.bottom - 24f * dp, 1.5f)
        val share = actions.top - play.bottom - 16f * dp
        assertEquals("the action row as far above the bottom", share, root.bottom - actions.bottom - 16f * dp, 1.5f)
        val (times, stars) = gaps(stars = true)
        assertTrue("a real gap ($times)", times > 20f * dp)
        assertEquals("stars to wave as times to transport", times, stars, 1f * dp)
        assertTrue("the stars' touch row clear of the seek bar's", bounds("rating").bottom <= bounds("player.seekBar").top + 0.5f)
        val controls = listOf("player.seekBar", "player.playPause", "player.mode.queue").map { bounds(it).top }
        for (mode in listOf("player.mode.queue", "player.mode.lyrics")) {
            showMode(mode)
            val (t, artist) = gaps(stars = false)
            assertEquals("artist line to wave as times to transport in $mode", t, artist, 1f * dp)
            listOf("player.seekBar", "player.playPause", "player.mode.queue").zip(controls).forEach { (tag, top) -> assertEquals("$tag stays put", top, bounds(tag).top, 1f) }
            showMode(mode)
        }
        compose.mainClock.autoAdvance = true
        core.client.dispatch(Commands.setSetting(SettingKeys.DISPLAY_PLAYER_RATING, "false"))
        compose.waitUntil(5_000) { !exists("rating") }
        compose.waitForIdle()
        val (t, artist) = gaps(stars = false)
        assertEquals("artist line to wave as times to transport without the stars", t, artist, 1f * dp)
    }

    @Test
    fun atTheReferenceSizeTheGapsAroundTheSeekBarMatch() {
        startExpanded()
        val root = compose.onNodeWithTag("nowPlaying.sheet").fetchSemanticsNode().boundsInRoot
        assertTrue("no taller than the reference (${root.height / dp} dp)", root.height <= 891f * dp)
        assertControlsLayout()
    }

    /**
     * A taller phone: the controls (title to actions) keep their largest gaps (40 dp from the times
     * to the transport), packed at the bottom; what is left goes to the mode area, the artwork
     * centred in it, and in queue mode the panel takes it.
     */
    @Test
    @Config(qualifiers = "w411dp-h960dp")
    fun onATallPhoneTheControlsStayAtTheBottomAndTheArtworkCentresAbove() {
        startExpanded()
        val handle = bounds("player.collapse")
        val art = bounds("player.artwork")
        assertTrue("the artwork lower than its margin under the handle ($art)", art.top - handle.bottom > 20f * dp)
        assertEquals("the largest gap", 40f * dp, gaps(stars = true).first, 1f * dp)
        tap("player.mode.queue")
        compose.waitUntil(5_000) { exists("queue.list") }
        compose.mainClock.advanceTimeBy(1_500)
        compose.waitForIdle()
        val panel = bounds("player.panel")
        assertEquals("the queue's panel from the handle", handle.bottom + 8f * dp, panel.top, 1f)
        assertTrue("taking the extra height ($panel)", panel.height >= art.height + 2f * (art.top - handle.bottom - 8f * dp) - 2f * dp)
        tap("player.mode.queue")
        compose.waitUntil(5_000) { !exists("queue.list") }
        compose.mainClock.advanceTimeBy(1_500)
        compose.waitForIdle()
        assertControlsLayout()
    }

    /** The add button shares the thumbnail's centre line: the title + artist pair's, in every mode, with or without the stars. */
    @Test
    fun theAddButtonCentresOnTheTitleAndArtist() {
        startExpanded()
        val track = core.client.nowPlaying.value!!.track
        fun check(what: String) {
            val pair = ((baseline(track.title) - titleCapHeight()) + baseline(track.artist!!)) / 2f
            assertEquals("the add button on the pair's centre ($what)", pair, bounds("player.addToPlaylist").center.y, 2f)
        }
        check("artwork")
        for (mode in listOf("player.mode.queue", "player.mode.lyrics")) {
            showMode(mode)
            check(mode)
            assertEquals("the thumbnail on the same line ($mode)", bounds("player.addToPlaylist").center.y, bounds("player.artwork").center.y, 2f)
            showMode(mode)
        }
        compose.mainClock.autoAdvance = true
        core.client.dispatch(Commands.setSetting(SettingKeys.DISPLAY_PLAYER_RATING, "false"))
        compose.waitUntil(5_000) { !exists("rating") }
        compose.waitForIdle()
        check("no stars")
    }

    /** Opens the player on a song whose title is far too long for the line. */
    private fun startWithLongTitle(reducedMotion: Boolean) {
        val lib = core.fake.library
        lib.tracks[0] = lib.tracks[0].copy(title = LONG_TITLE)
        compose.setThemedContent(core) {
            androidx.compose.runtime.CompositionLocalProvider(app.hocket.ui.a11y.LocalReducedMotion provides reducedMotion) { AppRoot(core.client) }
        }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        core.client.dispatch(Commands.playTracks(lib.serverId, listOf(lib.tracks[0].id), 0, "Long"))
        compose.waitUntil(5_000) { core.client.nowPlaying.value?.track?.title == LONG_TITLE }
        compose.waitUntil(5_000) { exists("miniPlayer") }
        compose.onNodeWithTag("nowPlaying.sheet").performSemanticsAction(SemanticsActions.Expand)
        compose.waitUntil(5_000) { exists("player.playPause") }
        compose.waitForIdle()
    }

    /** The title's text layout in the title block. */
    private fun titleLayout(): androidx.compose.ui.text.TextLayoutResult {
        val block = bounds("player.title")
        val node = compose.onAllNodes(androidx.compose.ui.test.hasText(LONG_TITLE), useUnmergedTree = true).fetchSemanticsNodes()
            .first { it.config.contains(SemanticsActions.GetTextLayoutResult) && block.contains(it.boundsInRoot.center) }
        val results = mutableListOf<androidx.compose.ui.text.TextLayoutResult>()
        node.config[SemanticsActions.GetTextLayoutResult].action!!.invoke(results)
        return results.first()
    }

    @Test
    fun aLongTitleIsOneLineAndScrolls() {
        startWithLongTitle(reducedMotion = false)
        assertEquals("one line", 1, titleLayout().lineCount)
        assertTrue("wider than the line, unshortened", titleLayout().size.width > bounds("player.title").width && !titleLayout().isLineEllipsized(0))
        assertTrue("scrolling", compose.onAllNodesWithTag("player.title.marquee", useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty())
        assertTrue("the whole title for TalkBack", compose.onNodeWithTag("player.title").fetchSemanticsNode().config[SemanticsProperties.ContentDescription].single().contains(LONG_TITLE))
    }

    @Test
    fun withReducedMotionALongTitleIsEllipsisedInstead() {
        startWithLongTitle(reducedMotion = true)
        assertEquals("one line", 1, titleLayout().lineCount)
        assertTrue("ellipsised", titleLayout().isLineEllipsized(0))
        assertFalse("not scrolling", compose.onAllNodesWithTag("player.title.marquee", useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty())
    }

    @Test
    fun theAlbumStaysBesideTheThumbnailInTheOtherModes() {
        startExpanded()
        val album = core.client.nowPlaying.value!!.track.album
        // The lyrics animate every frame while shown: step the clock by hand instead of waiting for idle.
        compose.mainClock.autoAdvance = false
        try {
            for (mode in listOf("lyrics", "queue")) {
                tap("player.mode.$mode")
                compose.mainClock.advanceTimeBy(1_000)
                assertTrue("the panel in $mode", exists("player.panel"))
                val node = compose.onAllNodesWithTag("player.album", useUnmergedTree = true).fetchSemanticsNodes().single()
                if (album != null) assertEquals(album, node.config[SemanticsProperties.Text].joinToString { it.text })
                // The thumbnail sits beside the title, left of it.
                assertTrue(bounds("player.artwork").right <= bounds("player.title").left + 1f)
                tap("player.mode.$mode")
                compose.mainClock.advanceTimeBy(1_000)
            }
        } finally {
            compose.mainClock.autoAdvance = true
        }
    }

    @Test
    fun theQueueNameOpensTheQueueAndItsChevronTheSwitcherWherePickingAQueueRestoresIt() {
        startExpanded()
        val label = core.client.queue.value.contextLabel!!
        assertEquals("Queue: $label", compose.onNodeWithTag("player.mode.queue").fetchSemanticsNode().config[SemanticsProperties.ContentDescription].single())
        // The name: the queue, in the artwork's place; again: back to the artwork.
        tap("player.mode.queue")
        compose.waitUntil(5_000) { exists("queue.list") }
        assertEquals(true, compose.onNodeWithTag("player.mode.queue").fetchSemanticsNode().config[SemanticsProperties.Selected])
        assertTrue("the toggles at the bottom of the panel", bounds("queue.controls").bottom <= bounds("player.panel").bottom + 1f && bounds("queue.controls").top > bounds("queue.list").top)
        tap("player.mode.queue")
        compose.waitUntil(5_000) { !exists("queue.list") }
        // The chevron: the switcher.
        tap("player.switchQueue")
        compose.waitUntil(5_000) { exists("queueSwitcher.sheet") }
        val saved = core.client.savedQueues.value.first { it.pinned }
        compose.waitUntil(5_000) { exists("savedQueue.${saved.id}") }
        tap("savedQueue.${saved.id}")
        compose.waitUntil(5_000) { !exists("queueSwitcher.sheet") }
        compose.waitUntil(5_000) { core.client.queue.value.contextLabel == saved.label }
        compose.waitForIdle()
        assertEquals("Queue: ${saved.label}", compose.onNodeWithTag("player.mode.queue").fetchSemanticsNode().config[SemanticsProperties.ContentDescription].single())
    }

    /** The device button's fill: a pixel inside its edge, away from the icon. */
    private fun deviceFill(): Int {
        val image = compose.onNodeWithTag("player.connect").captureToImage().asAndroidBitmap()
        return image.getPixel((image.width * 0.18f).toInt(), image.height / 2)
    }

    @Test
    fun playingOnShowsOnlyWhileAnotherDevicePlaysAndTheDeviceButtonTurnsAccent() {
        startExpanded()
        assertTrue(core.client.ownsTransport.value)
        assertFalse("no \"Playing on\" while this device plays", exists("player.playingOn"))
        assertTrue(compose.onAllNodesWithText("Playing on", substring = true).fetchSemanticsNodes().isEmpty())
        assertEquals(null, compose.onNodeWithTag("player.connect").fetchSemanticsNode().config.getOrNull(SemanticsProperties.StateDescription))
        val localFill = deviceFill()
        core.client.dispatch(Commands.handoffTo("laptop"))
        // The client reads who owns the transport from snapshots.
        core.client.requestSnapshot()
        compose.waitUntil(5_000) { !core.client.ownsTransport.value }
        compose.mainClock.advanceTimeBy(1_000)
        compose.waitForIdle()
        val laptop = core.client.devices.value.first { it.id == "laptop" }.name
        assertEquals("Playing on $laptop", compose.onNodeWithTag("player.playingOn", useUnmergedTree = true).fetchSemanticsNode().config[SemanticsProperties.Text].joinToString { it.text })
        assertEquals("Playing on $laptop", compose.onNodeWithTag("player.connect").fetchSemanticsNode().config[SemanticsProperties.StateDescription])
        val remoteFill = deviceFill()
        assertNotEquals("the device button takes the accent", localFill, remoteFill)
        val primary = remoteFill
        assertTrue("an opaque accent fill", android.graphics.Color.alpha(primary) == 255)
        tap("player.connect")
        compose.waitUntil(5_000) { exists("handoff.sheet") }
    }

    /** The first baseline (px, in the root) of the title block's line reading [text], from its text layout. */
    private fun baseline(text: String): Float {
        // In the title block (the queue below lists the same song).
        val block = bounds("player.title")
        val node = compose.onAllNodes(androidx.compose.ui.test.hasText(text), useUnmergedTree = true).fetchSemanticsNodes()
            .first { it.config.contains(SemanticsActions.GetTextLayoutResult) && block.contains(it.boundsInRoot.center) }
        val results = mutableListOf<androidx.compose.ui.text.TextLayoutResult>()
        node.config[SemanticsActions.GetTextLayoutResult].action!!.invoke(results)
        return node.positionInRoot.y + results.first().firstBaseline
    }

    /** The system sans-serif bold's cap height at the title's size (px), as the player works it out. */
    private fun titleCapHeight(): Float {
        val px = 24f * compose.activity.resources.displayMetrics.scaledDensity
        val paint = android.graphics.Paint().apply { typeface = android.graphics.Typeface.create(android.graphics.Typeface.SANS_SERIF, android.graphics.Typeface.BOLD); textSize = px }
        val r = android.graphics.Rect()
        paint.getTextBounds("H", 0, 1, r)
        return -r.top.toFloat()
    }

    /**
     * The stars show over the artwork only, right under the artist line; lyrics and the queue
     * leave the title block to the thumbnail, the title and the artist.
     */
    @Test
    fun theStarsShowOverTheArtworkOnly() {
        startExpanded()
        assertTrue(exists("rating"))
        assertEquals("stars a little under the title block", bounds("player.title").bottom + 1f * dp, bounds("rating").top, 1f)
        compose.mainClock.autoAdvance = false
        try {
            for (mode in listOf("queue", "lyrics")) {
                tap("player.mode.$mode")
                compose.mainClock.advanceTimeBy(1_500)
                assertFalse("no stars in $mode", exists("rating"))
                tap("player.mode.$mode")
                compose.mainClock.advanceTimeBy(1_500)
                assertTrue("stars back over the artwork", exists("rating"))
            }
        } finally {
            compose.mainClock.autoAdvance = true
        }
    }

    /** The seek bar, the transport and the action row stay put as the mode changes (the stars fold into the area). */
    @Test
    fun theControlsStayPutAcrossModes() {
        startExpanded()
        val tags = listOf("player.seekBar", "player.playPause", "player.mode.queue")
        val before = tags.map(::bounds)
        tap("player.mode.queue")
        compose.waitUntil(5_000) { exists("queue.list") }
        compose.mainClock.advanceTimeBy(1_500)
        compose.waitForIdle()
        tags.zip(before).forEach { (tag, b) -> assertEquals("$tag stays put", b.top, bounds(tag).top, 1f) }
    }

    /**
     * In lyrics and queue modes the thumbnail is 48 dp with the title and artist (their cap height
     * to the artist's baseline) centred on it.
     */
    @Test
    fun theThumbnailCentresTheTitleAndArtist() {
        startExpanded()
        val track = core.client.nowPlaying.value!!.track
        tap("player.mode.queue")
        compose.waitUntil(5_000) { exists("queue.list") }
        compose.mainClock.advanceTimeBy(1_500)
        compose.waitForIdle()
        val thumb = bounds("player.artwork")
        val capTop = baseline(track.title) - titleCapHeight()
        val artistBaseline = baseline(track.artist!!)
        assertTrue("the pair spans under 48 dp", artistBaseline - capTop < 48f * dp)
        assertEquals("48 dp", 48f * dp, thumb.height, 1f)
        assertEquals("square", thumb.width, thumb.height, 1f)
        assertEquals("the pair centred on the thumbnail", (capTop + artistBaseline) / 2f, thumb.center.y, 2f)
    }

    @Test
    fun anotherDevicesResumeOfferIsNotShownInThePlayer() {
        startExpanded()
        assertTrue("the fake core offers a resume", core.client.resumeOffer.value != null)
        assertFalse(exists("player.resumeHere"))
        assertTrue(compose.onAllNodesWithText("was playing", substring = true).fetchSemanticsNodes().isEmpty())
        assertTrue(compose.onAllNodesWithText("Resume here").fetchSemanticsNodes().isEmpty())
    }

    @Test
    fun lyricsAndTheQueueSitInTheArtworksSlotAcrossThePage() {
        startExpanded()
        val art = bounds("player.artwork")
        tap("player.mode.queue")
        compose.waitUntil(5_000) { exists("queue.list") }
        compose.waitForIdle()
        val slot = bounds("player.panel")
        val page = compose.onNodeWithTag("player.page").fetchSemanticsNode().boundsInRoot
        assertEquals("the page's width", page.width, slot.width, 1f)
        assertEquals("the artwork's top", art.top, slot.top, 2f)
        // The artwork's height, plus the room the folded stars gave it.
        assertTrue("the artwork's height and the stars' room: ${slot.height} vs ${art.height}", slot.height - art.height in -2f..(44f * dp))
    }

    /**
     * The timing row under the lyrics lives in the song menu while the lyrics show: "Lyrics timing"
     * opens a sheet with the tier and source and the offset buttons, which work as before.
     */
    @Test
    fun theLyricsTimingIsInTheSongMenuWhileTheLyricsShow() {
        startExpanded()
        // Not offered over the artwork.
        tap("player.more")
        compose.waitUntil(5_000) { exists("songMenu.header") }
        assertFalse(exists("player.lyricsTiming"))
        compose.onNodeWithTag("songMenu.header").performTouchInput { swipe(start = center, end = center + Offset(0f, 1200f), durationMillis = 100) }
        compose.waitUntil(5_000) { !exists("songMenu.header") }
        val trackId = core.client.nowPlaying.value!!.track.id
        // The lyrics animate every frame while shown: step the clock by hand.
        compose.mainClock.autoAdvance = false
        try {
            tap("player.mode.lyrics")
            compose.mainClock.advanceTimeBy(1_500)
            assertTrue(exists("lyrics.list"))
            assertFalse("no timing row under the lyrics", exists("lyrics.offset.later"))
            tap("player.more")
            repeat(60) { compose.mainClock.advanceTimeByFrame() }
            assertTrue(exists("player.lyricsTiming"))
            tap("player.lyricsTiming")
            repeat(90) { compose.mainClock.advanceTimeByFrame() }
            assertTrue(exists("lyrics.timing"))
            val before = core.client.lyrics.value[trackId]?.offsetMs ?: 0
            tap("lyrics.offset.later")
            repeat(30) { compose.mainClock.advanceTimeByFrame() }
            assertEquals(before + 100, core.client.lyrics.value[trackId]?.offsetMs)
            assertEquals("Show lyrics 0.1 seconds later", compose.onNodeWithTag("lyrics.offset.later").fetchSemanticsNode().config[SemanticsProperties.ContentDescription].single())
        } finally {
            compose.mainClock.autoAdvance = true
        }
    }

    @Test
    fun tappingTheTitleOpensTheSongsDetails() {
        startExpanded()
        val title = compose.onNodeWithTag("player.title").fetchSemanticsNode()
        assertEquals("About this song", title.config[SemanticsActions.OnClick].label)
        tap("player.title")
        compose.waitUntil(5_000) { exists("player.about") }
        compose.waitUntil(5_000) { compose.onAllNodesWithText("Format").fetchSemanticsNodes().isNotEmpty() }
    }

    private fun rating() = core.client.nowPlaying.value!!.track.rating.toInt()

    @Test
    fun draggingAlongTheStarsSetsTheRatingAndTappingItAgainClearsIt() {
        startExpanded()
        val initial = rating()
        assertNotEquals(4, initial)
        // Star n's centre: four 20 dp stars with 8 dp gaps.
        val star = { n: Int -> (n - 1) * 28f * dp + 10f * dp }
        compose.onNodeWithTag("rating").performTouchInput {
            down(Offset(star(1), height / 2f))
            moveTo(Offset(star(2), height / 2f))
            moveTo(Offset(star(3), height / 2f))
            moveTo(Offset(star(4), height / 2f))
        }
        compose.waitForIdle()
        // Shown under the finger, sent only on release.
        assertEquals(initial, rating())
        compose.onNodeWithTag("rating").performTouchInput { up() }
        compose.waitUntil(5_000) { rating() == 4 }
        // A tap on the current rating's star clears it; a tap elsewhere sets that star.
        compose.onNodeWithTag("rating").performTouchInput { click(Offset(star(4), height / 2f)) }
        compose.waitUntil(5_000) { rating() == 0 }
        compose.onNodeWithTag("rating").performTouchInput { click(Offset(star(2), height / 2f)) }
        compose.waitUntil(5_000) { rating() == 2 }
        // Dragging left of the first star clears it.
        compose.onNodeWithTag("rating").performTouchInput {
            down(Offset(star(2), height / 2f))
            moveTo(Offset(star(1), height / 2f))
            moveTo(Offset(-20f * dp, height / 2f))
            up()
        }
        compose.waitUntil(5_000) { rating() == 0 }
    }

    @Test
    fun thePlayerRatingSettingHidesTheStarsInThePlayerOnly() {
        startExpanded()
        assertTrue(exists("rating"))
        core.client.dispatch(Commands.setSetting(SettingKeys.DISPLAY_PLAYER_RATING, "false"))
        compose.waitUntil(5_000) { !exists("rating") }
        // Still in the song's details and its menu.
        tap("player.title")
        compose.waitUntil(5_000) { exists("player.about") }
        compose.waitUntil(5_000) { exists("rating") }
        core.client.dispatch(Commands.setSetting(SettingKeys.DISPLAY_PLAYER_RATING, "true"))
        compose.waitUntil(5_000) { compose.onAllNodesWithTag("rating").fetchSemanticsNodes().size == 2 }
    }

    /** Puts a finger on the artwork and drags it left by [by] px (past the touch slop). */
    private fun dragArtwork(by: Float) {
        compose.onNodeWithTag("player.artwork").performTouchInput {
            down(center)
            moveBy(Offset(-viewConfiguration.touchSlop - 1f, 0f))
            repeat(10) { moveBy(Offset(-by / 10f, 0f)) }
        }
        compose.waitForIdle()
    }

    @Test
    fun halfASwipeShowsTheNextCoverAndSkipsNothing() {
        startExpanded()
        val before = core.client.nowPlaying.value!!.item.key
        val next = core.client.queue.value.let { it.playingNext.firstOrNull() ?: it.upcoming.first() }
        assertFalse("the next cover waits off screen", displayed("player.artwork.next"))
        val width = bounds("player.artwork").width
        // Slowly, under half the width: the next cover peeks in.
        dragArtwork(width * 0.4f)
        assertTrue("the next cover peeks in", displayed("player.artwork.next"))
        assertTrue(bounds("player.artwork.next").left < width)
        compose.mainClock.advanceTimeBy(500)
        compose.onNodeWithTag("player.artwork").performTouchInput { up() }
        compose.mainClock.advanceTimeBy(1_000)
        compose.waitForIdle()
        assertEquals("nothing skipped", before, core.client.nowPlaying.value!!.item.key)
        assertNotEquals(next.item.key, core.client.nowPlaying.value!!.item.key)
        assertFalse("sprung back", displayed("player.artwork.next"))
    }

    @Test
    fun aFullSwipeSkipsToTheNextSongOnce() {
        startExpanded()
        val q = core.client.queue.value
        val next = q.playingNext.firstOrNull() ?: q.upcoming.first()
        val history = q.history.size
        val width = bounds("player.artwork").width
        dragArtwork(width * 0.7f)
        compose.mainClock.advanceTimeBy(500)
        compose.onNodeWithTag("player.artwork").performTouchInput { up() }
        compose.waitUntil(5_000) { core.client.nowPlaying.value?.item?.key == next.item.key }
        compose.mainClock.advanceTimeBy(1_500)
        compose.waitForIdle()
        assertEquals("one skip", next.item.key, core.client.nowPlaying.value!!.item.key)
        assertEquals(history + 1, core.client.queue.value.history.size)
        // The strip is back under the new current cover.
        assertTrue(displayed("player.artwork"))
        assertFalse(displayed("player.artwork.next"))
    }

    @Test
    fun atTheEndOfTheQueueThereIsNoNextCoverAndTheSwipeSpringsBack() {
        startExpanded()
        val track = core.client.nowPlaying.value!!.track.id
        core.client.dispatch(Commands.setAutoplay(false))
        core.client.dispatch(Commands.playTracks(core.fake.library.serverId, listOf(track), 0, "One track"))
        compose.waitUntil(5_000) { core.client.queue.value.upcoming.isEmpty() && core.client.nowPlaying.value?.track?.id == track }
        compose.waitForIdle()
        val rest = bounds("player.artwork")
        // Under the rubber band's threshold: no neighbour, the cover lags the finger, springs back.
        dragArtwork(60f * dp)
        assertFalse(exists("player.artwork.next"))
        compose.onNodeWithTag("player.artwork").performTouchInput { up() }
        compose.mainClock.advanceTimeBy(1_500)
        compose.waitForIdle()
        assertEquals(rest.left, bounds("player.artwork").left, 1f)
        assertEquals(track, core.client.nowPlaying.value!!.track.id)
    }

    private companion object {
        const val LONG_TITLE = "The Lighthouse Keeper's Daughter (Live at the Harbour Room)"
    }
}
