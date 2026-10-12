package app.hocket.ui

import androidx.activity.ComponentActivity
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipe
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.Commands
import app.hocket.core.api.Command
import app.hocket.ui.nav.AppRoot
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * The now-playing sheet's open and dismiss paths: the drag up from the mini bar, the drag down
 * through the player's nested scroll, the back gesture (predictive back), the mini bar's skip swipe,
 * with playback running and paused, and repeatedly, so a dismissal never reads state after the
 * sheet has left composition or trips over a null current item.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class NowPlayingSheetTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private fun displayed(tag: String): Boolean = runCatching { compose.onNodeWithTag(tag).assertIsDisplayed() }.isSuccess

    private fun start(core: TestCore) {
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        compose.onNodeWithTag("miniPlayer").assertIsDisplayed()
    }

    private fun expand() {
        // Drag the mini bar up: a fast, long swipe starting inside its bounds.
        compose.onNodeWithTag("miniPlayer").performTouchInput { swipe(start = center, end = center - Offset(0f, 1200f), durationMillis = 150) }
        compose.waitUntil(5_000) { displayed("player.playPause") }
        compose.onNodeWithTag("player.title").assertIsDisplayed()
    }

    private fun collapseByDrag() {
        // Drag the expanded content down (through the nested scroll connection) to collapse.
        compose.onNodeWithTag("player.title").performTouchInput { swipe(start = center, end = center + Offset(0f, 1200f), durationMillis = 150) }
        compose.waitUntil(5_000) { !displayed("player.playPause") }
        compose.onNodeWithTag("miniPlayer").assertIsDisplayed()
    }

    @Test
    fun miniPlayerExpandsOnSwipeUpAndCollapsesOnSwipeDown() {
        val core = TestCore()
        start(core)
        assertTrue(core.client.isPlaying.value)
        expand()
        collapseByDrag()
    }

    @Test
    fun dismissingTheSheetWhilePausedWorksToo() {
        val core = TestCore()
        start(core)
        core.client.dispatch(Command.Pause)
        compose.waitUntil(5_000) { !core.client.isPlaying.value }
        expand()
        collapseByDrag()
        // And back up and down again on the same sheet state.
        expand()
        collapseByDrag()
        assertFalse(core.client.isPlaying.value)
    }

    @Test
    fun theBackGestureCollapsesTheExpandedSheet() {
        val core = TestCore()
        start(core)
        expand()
        compose.activityRule.scenario.onActivity { it.onBackPressedDispatcher.onBackPressed() }
        compose.waitUntil(5_000) { !displayed("player.playPause") }
        compose.onNodeWithTag("miniPlayer").assertIsDisplayed()
        // With the sheet collapsed, back is no longer intercepted: the sheet stays collapsed and the
        // shell is still there.
        compose.activityRule.scenario.onActivity { it.onBackPressedDispatcher.onBackPressed() }
        compose.waitForIdle()
        compose.onNodeWithTag("miniPlayer").assertIsDisplayed()
        // And the sheet still opens afterwards.
        expand()
        collapseByDrag()
    }

    /** A one-track queue with autoplay off, so skipping past it leaves nothing current. */
    private fun playOnlyOneTrack(core: TestCore): String {
        val track = core.client.nowPlaying.value!!.track.id
        core.client.dispatch(Commands.setAutoplay(false))
        core.client.dispatch(Commands.playTracks(core.fake.library.serverId, listOf(track), 0, "One track"))
        compose.waitUntil(5_000) { core.client.queue.value.upcoming.isEmpty() && core.client.nowPlaying.value?.track?.id == track }
        return track
    }

    @Test
    fun swipingTheMiniPlayerBackThenPastTheEndOfTheQueueDismissesItWithoutCrashing() {
        val core = TestCore()
        start(core)
        val track = playOnlyOneTrack(core)
        // Back on the first track: restarts it, the bar stays.
        compose.onNodeWithTag("miniPlayer").performTouchInput { swipe(start = center, end = center + Offset(300f, 0f), durationMillis = 150) }
        compose.waitForIdle()
        compose.onNodeWithTag("miniPlayer").assertIsDisplayed()
        assertEquals(track, core.client.nowPlaying.value!!.track.id)
        // Forward on the last track: nothing is current any more and the player goes away mid-gesture.
        compose.onNodeWithTag("miniPlayer").performTouchInput { swipe(start = center, end = center - Offset(300f, 0f), durationMillis = 150) }
        compose.waitUntil(5_000) { core.client.nowPlaying.value == null }
        compose.waitForIdle()
        assertFalse(displayed("miniPlayer"))
        // And it comes back for the next thing played.
        core.client.dispatch(Commands.playTracks(core.fake.library.serverId, listOf(track), 0, "Again"))
        compose.waitUntil(5_000) { displayed("miniPlayer") }
        expand()
        collapseByDrag()
    }

    private val modes = listOf("lyrics", "queue")
    /** The queue's name is a button marked selected while the queue shows; lyrics a toggle, on while shown. */
    private fun modeSelected(mode: String): Boolean = compose.onNodeWithTag("player.mode.$mode").fetchSemanticsNode().config.let { c ->
        c.getOrElse(SemanticsProperties.Selected) { false } || c.getOrNull(SemanticsProperties.ToggleableState) == androidx.compose.ui.state.ToggleableState.On
    }

    /**
     * A semantics click on [tag], then the state it wrote handed to composition: with the clock under
     * manual control nothing else sends the global snapshot's apply notifications (a device's looper would).
     */
    private fun tap(tag: String) {
        compose.onNodeWithTag(tag).performSemanticsAction(SemanticsActions.OnClick)
        compose.runOnUiThread { androidx.compose.runtime.snapshots.Snapshot.sendApplyNotifications() }
    }

    /** Switches the full player to [mode] (lyrics / queue), or back to the artwork for null. */
    private fun openMode(mode: String?) {
        if (mode == null) {
            // Tapping the selected one again goes back to the artwork.
            modes.filter { modeSelected(it) }.forEach { tap("player.mode.$it") }
            advanceUntil("artwork mode") { modes.none { modeSelected(it) } }
        } else {
            if (!modeSelected(mode)) tap("player.mode.$mode")
            advanceUntil("mode $mode") { modeSelected(mode) }
        }
    }

    /**
     * Frame by frame, for pages that animate forever while shown (the lyrics' frame loop): with the
     * clock under manual control nothing waits for an idle that never comes.
     */
    private fun advanceUntil(what: String, condition: () -> Boolean) {
        repeat(600) { if (condition()) return; compose.mainClock.advanceTimeByFrame() }
        throw AssertionError("timed out waiting for $what (sheet top ${runCatching { sheetTop() }.getOrNull()}, content top ${contentTop()})")
    }

    private fun sheetTop(): Float = compose.onNodeWithTag("nowPlaying.sheet").fetchSemanticsNode().boundsInRoot.top
    /** Where the app's content starts: below the debug fake-core banner, which is laid out in flow above it. */
    private fun contentTop(): Float = runCatching { compose.onNodeWithTag("debug.fakeCoreBanner").fetchSemanticsNode().boundsInRoot.bottom }.getOrDefault(0f)
    private fun sheetOpen(): Boolean = sheetTop() <= contentTop() + 1f
    private fun sheetCollapsed(): Boolean = displayed("miniPlayer") && sheetTop() > compose.onRoot().fetchSemanticsNode().size.height / 2f

    /** A vertical drag starting near the top of [tag], [by] px down over [millis]. */
    private fun dragDown(tag: String, by: Float, millis: Long) {
        compose.onNodeWithTag(tag).performTouchInput { swipe(start = Offset(centerX, top + 40f), end = Offset(centerX, top + 40f + by), durationMillis = millis) }
    }

    /**
     * The swipe-away paths from a scrolling page at its top (the page cannot scroll up, so the
     * sheet takes the drag and the release's fling through the nested-scroll connection): a short
     * slow drag springs back open, a slow drag past the threshold and a fast fling both dismiss.
     * The release used to call the deprecated `AnchoredDraggableState.settle(velocity)`, which
     * throws for this state: the swipe-away crash.
     */
    private fun swipeAwayFrom(mode: String?, tag: String) {
        compose.mainClock.autoAdvance = false
        try {
            val height = compose.onNodeWithTag("nowPlaying.sheet").fetchSemanticsNode().size.height.toFloat()
            fun open() {
                if (!sheetOpen()) compose.onNodeWithTag("miniPlayer.info").performSemanticsAction(SemanticsActions.OnClick)
                advanceUntil("the sheet to open") { sheetOpen() }
                openMode(mode)
                advanceUntil("$tag to show") { displayed(tag) }
            }
            open()
            // Short and slow: under both thresholds, back to open.
            dragDown(tag, height * 0.12f, 3_000)
            advanceUntil("the sheet to settle open") { sheetOpen() }
            // Slow but past the positional threshold: dismissed.
            dragDown(tag, height * 0.5f, 4_000)
            advanceUntil("the sheet to settle collapsed") { sheetCollapsed() }
            // Open again (while the page's own fling may still be running: its late settle must
            // not interrupt the expand) and fling it away.
            open()
            dragDown(tag, height * 0.2f, 60)
            advanceUntil("the fling to dismiss the sheet") { sheetCollapsed() }
        } finally {
            compose.mainClock.autoAdvance = true
        }
    }

    private fun swipeAwayFromEveryPage(core: TestCore) {
        swipeAwayFrom("queue", "queue.list")
        swipeAwayFrom("lyrics", "lyrics.list")
        swipeAwayFrom(null, "player.page")
        assertTrue(core.client.nowPlaying.value != null)
    }

    @Test
    fun swipingThePlayerAwayFromAnyPageWhilePlaying() {
        val core = TestCore()
        start(core)
        assertTrue(core.client.isPlaying.value)
        swipeAwayFromEveryPage(core)
    }

    @Test
    fun swipingThePlayerAwayFromAnyPageWhileIdle() {
        val core = TestCore()
        start(core)
        core.client.dispatch(Command.Pause)
        compose.waitUntil(5_000) { !core.client.isPlaying.value }
        swipeAwayFromEveryPage(core)
        assertFalse(core.client.isPlaying.value)
    }

    @Test
    fun releaseTargetsFollowTheThresholds() {
        val state = app.hocket.ui.player.NowPlayingSheetState(app.hocket.ui.player.SheetValue.Expanded)
        state.velocityThreshold = 100f
        // Anchors are not laid out: progress reads from the current value (1 = open).
        assertEquals(app.hocket.ui.player.SheetValue.Collapsed, state.targetFor(500f))
        assertEquals(app.hocket.ui.player.SheetValue.Expanded, state.targetFor(-500f))
        assertEquals(app.hocket.ui.player.SheetValue.Expanded, state.targetFor(20f))
    }

    @Test
    fun swipingThePlayerAwayAfterTheQueueRanOutWhileItWasOpen() {
        val core = TestCore()
        start(core)
        playOnlyOneTrack(core)
        expand()
        // Skip past the only track from the full player: nothing is current, the sheet stays open.
        compose.onNodeWithTag("player.next").performClick()
        compose.waitUntil(5_000) { core.client.nowPlaying.value == null }
        compose.waitForIdle()
        // Swiping it away with no current item removes the whole sheet.
        compose.onNodeWithTag("nowPlaying.sheet").performTouchInput { swipe(start = Offset(centerX, top + 300f), end = Offset(centerX, top + 1600f), durationMillis = 120) }
        compose.waitUntil(5_000) { !displayed("nowPlaying.sheet") }
        assertFalse(displayed("miniPlayer"))
    }

    @Test
    fun tappingTheMiniPlayerExpandsAndTransportWorks() {
        val core = TestCore()
        start(core)
        assertTrue(core.client.isPlaying.value)
        compose.onNodeWithTag("miniPlayer.playPause").performClick()
        compose.waitUntil(5_000) { !core.client.isPlaying.value }
        assertFalse(core.client.isPlaying.value)
        compose.onNodeWithTag("miniPlayer").performClick()
        compose.waitUntil(5_000) { displayed("player.next") }
        val before = core.client.nowPlaying.value!!.track.id
        compose.onNodeWithTag("player.next").performClick()
        compose.waitUntil(5_000) { core.client.nowPlaying.value?.track?.id != before }
    }

    @Test
    fun theModeStaysAcrossCollapsingAndExpandingAndTheThumbnailBringsTheArtworkBack() {
        val core = TestCore()
        start(core)
        expand()
        compose.mainClock.autoAdvance = false
        try {
            openMode("queue")
            advanceUntil("the queue") { displayed("queue.list") }
            // Collapse and expand again: still the queue (Apple Music keeps the mode).
            compose.onNodeWithTag("nowPlaying.sheet").performSemanticsAction(SemanticsActions.Dismiss)
            advanceUntil("the sheet to collapse") { sheetCollapsed() }
            compose.onNodeWithTag("miniPlayer.info").performSemanticsAction(SemanticsActions.OnClick)
            advanceUntil("the sheet to open") { sheetOpen() }
            advanceUntil("the queue again") { displayed("queue.list") && modeSelected("queue") }
            // The artwork, now a thumbnail beside the title, brings the artwork back when tapped.
            tap("player.artwork")
            advanceUntil("artwork mode") { modes.none { modeSelected(it) } }
            // And lyrics, toggled off again, does too.
            openMode("lyrics")
            advanceUntil("lyrics") { displayed("lyrics.page") }
            tap("player.mode.lyrics")
            advanceUntil("artwork mode again") { modes.none { modeSelected(it) } }
        } finally {
            compose.mainClock.autoAdvance = true
        }
    }

    /**
     * The immersive dismiss (#40 follow-up, "feels a bit janky"): the artwork in flight and its
     * continuation are one unit. Through the first part of the drag the scene keeps its full size
     * (sliding down with the sheet); then it shrinks, and wherever it is, the transform the
     * continuation is drawn with puts the continuation's edges and seam exactly on the artwork's.
     * The continuation has faded before the shrink starts, the seam stays the landed one until
     * then, and the handle has gone long before.
     */
    @Test
    fun anImmersiveArtworkInFlightCarriesItsContinuationAsOneUnit() {
        val hero = app.hocket.ui.player.HeroGeometry().apply {
            immersive = true
            headerHeight = 80f
            // The page puts the immersive artwork at its top (the header's height above the area).
            immersiveTop = -80f
            area = androidx.compose.ui.geometry.Rect(0f, 80f, 1080f, 1080f)
            thumb = androidx.compose.ui.geometry.Rect(46f, 30f, 172f, 156f)
        }
        val big = hero.bigSlot()
        assertEquals(androidx.compose.ui.geometry.Rect(0f, 0f, 1080f, 1080f), big)
        val none = androidx.compose.ui.geometry.Rect.Zero
        for (p in listOf(1f, 0.9f, 0.7f)) {
            assertEquals("full size at $p", big, hero.flightRect(p, 0f, 1f, none))
            assertEquals(1f, hero.flightTransform(p, 0f, 1f, none).scale)
        }
        val feather = app.hocket.ui.player.FEATHER
        for (p in listOf(0.5f, 0.3f)) {
            val r = hero.flightRect(p, 0f, 1f, none)
            assertTrue("shrinking at $p: $r", r.width < big.width && r.width > hero.thumb.width)
            val t = hero.flightTransform(p, 0f, 1f, none)
            fun x(v: Float) = v * t.scale + t.x
            fun y(v: Float) = v * t.scale + t.y
            assertEquals("left edges at $p", r.left, x(big.left), 0.5f)
            assertEquals("right edges at $p", r.right, x(big.right), 0.5f)
            assertEquals("tops at $p", r.top, y(big.top), 0.5f)
            // The continuation is laid out from the seam, FEATHER of the side above the bottom.
            assertEquals("seams at $p", r.bottom - feather * r.width, y(big.bottom - feather * big.width), 0.5f)
        }
        val shrinkFrom = app.hocket.ui.player.HeroGeometry.SHRINK_FROM
        assertEquals(0f, app.hocket.ui.player.HeroGeometry.continuationAlpha(shrinkFrom - 0.15f), 1e-4f)
        assertEquals(1f, app.hocket.ui.player.HeroGeometry.continuationAlpha(0.9f), 1e-4f)
        assertEquals(1f, app.hocket.ui.player.HeroGeometry.seamFraction(0.45f), 1e-4f)
        assertEquals(0f, app.hocket.ui.player.HeroGeometry.headerAlpha(0.85f), 1e-4f)
        assertEquals(1f, app.hocket.ui.player.HeroGeometry.headerAlpha(1f), 1e-4f)
        // A card is unchanged: it follows the progress from the start.
        hero.immersive = false
        hero.artSide = 1000f
        assertTrue(hero.flightRect(0.9f, 0f, 1f, none).width < hero.bigSlot().width)
    }

    /** The mini player's skip swipe is the carousel in miniature: half way the next song peeks in; further, it skips once. */
    @Test
    fun theMiniPlayerPeeksAtTheNextSongAndSkipsOncePastHalfWay() {
        val core = TestCore()
        start(core)
        val before = core.client.nowPlaying.value!!.item.key
        val next = core.client.queue.value.let { it.playingNext.firstOrNull() ?: it.upcoming.first() }
        val width = compose.onNodeWithTag("miniPlayer").fetchSemanticsNode().size.width.toFloat()
        assertFalse(displayed("miniPlayer.next"))
        compose.onNodeWithTag("miniPlayer").performTouchInput {
            down(center)
            moveBy(Offset(-viewConfiguration.touchSlop - 1f, 0f))
            repeat(8) { moveBy(Offset(-width * 0.25f / 8f, 0f)) }
        }
        compose.waitForIdle()
        assertTrue("the next song peeks in", displayed("miniPlayer.next"))
        compose.mainClock.advanceTimeBy(500)
        compose.onNodeWithTag("miniPlayer").performTouchInput { up() }
        compose.mainClock.advanceTimeBy(1_000)
        compose.waitForIdle()
        assertEquals("a slow short swipe skips nothing", before, core.client.nowPlaying.value!!.item.key)
        compose.onNodeWithTag("miniPlayer").performTouchInput {
            down(center)
            moveBy(Offset(-viewConfiguration.touchSlop - 1f, 0f))
            repeat(8) { moveBy(Offset(-width * 0.6f / 8f, 0f)) }
        }
        compose.mainClock.advanceTimeBy(500)
        compose.onNodeWithTag("miniPlayer").performTouchInput { up() }
        compose.waitUntil(5_000) { core.client.nowPlaying.value?.item?.key == next.item.key }
        compose.mainClock.advanceTimeBy(1_500)
        compose.waitForIdle()
        assertEquals("one skip", next.item.key, core.client.nowPlaying.value!!.item.key)
    }

    /** Square only at rest fully open; extra-large corners as soon as it is dragged; the mini card's when collapsed. */
    @Test
    fun theSheetsTopCornersRoundAsSoonAsItLeavesTheOpenRest() {
        val corner = { p: Float -> app.hocket.ui.player.sheetTopCorner(p, miniCorner = 16f, sheetCorner = 28f) }
        assertEquals(0f, corner(1f), 0f)
        assertTrue("rounding off the rest: ${corner(0.99f)}", corner(0.99f) > 5f)
        assertEquals(28f, corner(0.95f), 1f)
        assertEquals(22f, corner(0.5f), 0.5f)
        assertEquals(16f, corner(0f), 0f)
    }

    @Test
    fun theSheetStateSavesItsModeAndPosition() {
        val state = app.hocket.ui.player.NowPlayingSheetState(app.hocket.ui.player.SheetValue.Expanded, app.hocket.ui.player.PlayerMode.Lyrics)
        val saver = app.hocket.ui.player.NowPlayingSheetState.Saver
        val saved = with(saver) { androidx.compose.runtime.saveable.SaverScope { true }.save(state) }!!
        val restored = saver.restore(saved)!!
        assertEquals(app.hocket.ui.player.PlayerMode.Lyrics, restored.mode)
        assertEquals(app.hocket.ui.player.SheetValue.Expanded, restored.draggable.currentValue)
        // A value saved before modes existed restores to the artwork.
        assertEquals(app.hocket.ui.player.PlayerMode.Artwork, saver.restore("Collapsed")!!.mode)
        // About became a sheet: a state saved in About opens on the artwork.
        assertEquals(app.hocket.ui.player.PlayerMode.Artwork, saver.restore("Expanded|About")!!.mode)
    }
}
