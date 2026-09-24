package app.hocket.ui

import androidx.activity.ComponentActivity
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
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

    private val modes = listOf("lyrics", "queue", "about")
    private fun modeSelected(mode: String): Boolean = compose.onNodeWithTag("player.mode.$mode").fetchSemanticsNode().config.getOrElse(SemanticsProperties.Selected) { false }

    /** Switches the full player to [mode] (lyrics / queue / about), or back to the artwork for null. */
    private fun openMode(mode: String?) {
        if (mode == null) {
            // Tapping the selected pill again goes back to the artwork.
            modes.filter { modeSelected(it) }.forEach { compose.onNodeWithTag("player.mode.$it").performSemanticsAction(SemanticsActions.OnClick) }
            advanceUntil("artwork mode") { modes.none { modeSelected(it) } }
        } else {
            if (!modeSelected(mode)) compose.onNodeWithTag("player.mode.$mode").performSemanticsAction(SemanticsActions.OnClick)
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
        swipeAwayFrom("about", "player.about")
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
            compose.onNodeWithTag("player.artwork").performSemanticsAction(SemanticsActions.OnClick)
            advanceUntil("artwork mode") { modes.none { modeSelected(it) } }
            // And a selected pill tapped again does too.
            openMode("about")
            advanceUntil("about") { displayed("player.about") }
            compose.onNodeWithTag("player.mode.about").performSemanticsAction(SemanticsActions.OnClick)
            advanceUntil("artwork mode again") { modes.none { modeSelected(it) } }
        } finally {
            compose.mainClock.autoAdvance = true
        }
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
    }
}
