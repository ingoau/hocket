package app.hocket.ui.a11y

import androidx.activity.ComponentActivity
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performSemanticsAction
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.ui.TestCore
import app.hocket.ui.nav.AppRoot
import app.hocket.ui.setThemedContent
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * What TalkBack gets from the player: the mini player as one item with skip actions and a polite
 * live region that changes once per track (never with the position); the sheet's expand, collapse
 * and dismiss actions in place of the drag; the page behind a full-screen player out of reach; the
 * seek bar and rating as adjustable controls with spoken values.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class PlayerAccessibilityTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private var clock = 2_000_000.0
    private val core = TestCore(now = { clock })

    private fun start() {
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        compose.waitUntil(5_000) { core.client.started.value }
        compose.waitForIdle()
    }

    private fun node(tag: String): SemanticsNode = compose.onNodeWithTag(tag).fetchSemanticsNode()
    private fun exists(tag: String) = runCatching { compose.onNodeWithTag(tag).assertExists() }.isSuccess
    private fun SemanticsNode.desc() = config.getOrNull(SemanticsProperties.ContentDescription)?.joinToString(" ")
    private fun SemanticsNode.actions() = config.getOrNull(SemanticsActions.CustomActions)?.map { it.label } ?: emptyList()
    private fun all(): List<SemanticsNode> = A11yChecks.all(compose.onRoot().fetchSemanticsNode())
    private fun liveRegions() = all().filter { it.config.contains(SemanticsProperties.LiveRegion) }
    private fun sheet() = node("nowPlaying.sheet")
    private fun isInSheet(n: SemanticsNode): Boolean {
        var p: SemanticsNode? = n
        while (p != null) { if (p.config.getOrNull(SemanticsProperties.TestTag) == "nowPlaying.sheet") return true; p = p.parent }
        return false
    }

    @Test
    fun theMiniPlayerIsOneItemWithSkipActionsAndAnnouncesTrackChangesOnly() {
        start()
        val track = core.client.nowPlaying.value!!.track
        val info = node("miniPlayer.info")
        assertTrue(info.desc(), info.desc()!!.startsWith("Now playing: ${track.title} by ${track.artist}"))
        assertEquals(LiveRegionMode.Polite, info.config[SemanticsProperties.LiveRegion])
        assertEquals(listOf("Next track", "Previous track"), info.actions())
        assertEquals("Open now playing", info.config[SemanticsActions.OnClick].label)
        // Exactly one live region on screen, and the position line is not exposed at all.
        assertEquals(1, liveRegions().size)
        assertTrue("no progress semantics in the mini player", all().none { it.config.contains(SemanticsProperties.ProgressBarRangeInfo) })

        // Position ticks: seconds pass, the live region's text does not change (nothing to announce).
        repeat(5) {
            clock += 1_000.0
            compose.mainClock.advanceTimeBy(1_000)
            compose.waitForIdle()
            assertEquals(info.desc(), node("miniPlayer.info").desc())
        }

        // A track change changes it once: the skip swipe's accessible twin.
        compose.onNodeWithTag("miniPlayer.info").performCustomAction(compose, "Next track")
        compose.waitUntil(5_000) { core.client.nowPlaying.value?.track?.id != track.id }
        compose.waitForIdle()
        val next = core.client.nowPlaying.value!!.track
        assertTrue(node("miniPlayer.info").desc()!!.startsWith("Now playing: ${next.title} by ${next.artist}"))
        assertEquals(1, liveRegions().size)
    }

    @Test
    fun theSheetExpandsCollapsesAndDismissesWithoutTheDragAndHidesWhatIsBehindIt() {
        start()
        val sheet = sheet()
        assertTrue("collapsed: an expand action", sheet.config.contains(SemanticsActions.Expand))
        assertFalse("the full player is not in the tree while collapsed", exists("player.playPause"))
        assertTrue(exists("navBar.library"))

        compose.onNodeWithTag("nowPlaying.sheet").performSemanticsAction(SemanticsActions.Expand)
        compose.waitUntil(5_000) { exists("player.playPause") }
        compose.waitForIdle()
        val open = sheet()
        assertEquals("Now playing", open.config[SemanticsProperties.PaneTitle])
        assertTrue(open.config.contains(SemanticsActions.Collapse))
        assertTrue("a dismiss action for the swipe-down", open.config.contains(SemanticsActions.Dismiss))
        assertFalse("the mini player is gone from the tree", exists("miniPlayer.info"))
        assertFalse("the page and navigation behind the full player are out of reach", exists("navBar.library"))
        val behind = A11yChecks.all(compose.onRoot().fetchSemanticsNode()).filter { A11yChecks.actionable(it) && !isInSheet(it) }
        assertTrue("nothing actionable outside the sheet: ${behind.map { A11yChecks.describe(it) }}", behind.isEmpty())
        // The page's heading is the track, a polite live region (announced once per track).
        val track = core.client.nowPlaying.value!!.track
        val title = node("player.title")
        assertTrue(title.config.contains(SemanticsProperties.Heading))
        assertEquals(LiveRegionMode.Polite, title.config[SemanticsProperties.LiveRegion])
        assertEquals("${track.title} by ${track.artist}", title.desc())
        assertEquals("one live region: the page title", 1, liveRegions().size)

        compose.onNodeWithTag("nowPlaying.sheet").performSemanticsAction(SemanticsActions.Dismiss)
        compose.waitUntil(5_000) { !exists("player.playPause") }
        compose.waitForIdle()
        assertTrue(exists("miniPlayer.info"))
        assertTrue(exists("navBar.library"))
    }

    private fun expand() {
        compose.onNodeWithTag("nowPlaying.sheet").performSemanticsAction(SemanticsActions.Expand)
        compose.waitUntil(5_000) { exists("player.playPause") }
        compose.waitForIdle()
    }

    @Test
    fun theSeekBarSpeaksMinutesAndSecondsAndSeeksFromTheAdjustAction() {
        start()
        expand()
        val track = core.client.nowPlaying.value!!.track
        val durationS = (track.durationMs.toLong() / 1000).toFloat()
        val seek = node("player.seekBar")
        assertEquals("Playback position", seek.desc())
        val range = seek.config[SemanticsProperties.ProgressBarRangeInfo]
        assertEquals(0f, range.range.start)
        assertEquals(durationS, range.range.endInclusive)
        val resources = compose.activity.resources
        val state = seek.config[SemanticsProperties.StateDescription]
        assertEquals(Spoken.position(resources, (range.current * 1000).toLong(), track.durationMs.toLong()), state)
        assertTrue(state, Regex("""\d+ (second|minute)s?.* of \d+ minutes?( \d+ seconds?)?""").matches(state))
        // The screen reader's adjust gesture seeks.
        compose.onNodeWithTag("player.seekBar").performSemanticsAction(SemanticsActions.SetProgress) { it(92f) }
        compose.waitUntil(5_000) { core.client.transport.value.position.positionMs.toLong() in 91_500..92_500 }
        compose.waitForIdle()
        assertEquals("1 minute 32 seconds of " + Spoken.duration(resources, (durationS * 1000).toLong()), node("player.seekBar").config[SemanticsProperties.StateDescription])
    }

    @Test
    fun theRatingIsOneAdjustableControlThatSaysHowManyStars() {
        start()
        expand()
        val id = core.client.nowPlaying.value!!.track.id
        compose.onNodeWithTag("rating").performSemanticsAction(SemanticsActions.SetProgress) { it(3f) }
        compose.waitUntil(5_000) { core.client.nowPlaying.value?.track?.rating == 3u }
        compose.waitForIdle()
        val rating = node("rating")
        assertEquals("Rating", rating.desc())
        assertEquals("3 of 5 stars", rating.config[SemanticsProperties.StateDescription])
        assertEquals(3f, rating.config[SemanticsProperties.ProgressBarRangeInfo].current)
        assertEquals(4, rating.config[SemanticsProperties.ProgressBarRangeInfo].steps)
        assertTrue("the stars are not separate stops", rating.children.isEmpty())
        compose.onNodeWithTag("rating").performCustomAction(compose, "Clear rating")
        compose.waitUntil(5_000) { core.client.nowPlaying.value?.track?.rating == 0u }
        compose.waitForIdle()
        assertEquals("Not rated", node("rating").config[SemanticsProperties.StateDescription])
        assertEquals(id, core.client.nowPlaying.value!!.track.id)
    }

    @Test
    fun theArtworkOffersTheSwipeAndLongPressAsActions() {
        start()
        expand()
        val art = node("player.artwork")
        assertEquals(listOf("Next track", "Previous track"), art.actions().take(2))
        assertEquals(3, art.actions().size)
        assertNull("no do-nothing click on the artwork", art.config.getOrNull(SemanticsActions.OnClick))
        val before = core.client.nowPlaying.value!!.track.id
        compose.onNodeWithTag("player.artwork").performCustomAction(compose, "Next track")
        compose.waitUntil(5_000) { core.client.nowPlaying.value?.track?.id != before }
        assertNotEquals(before, core.client.nowPlaying.value!!.track.id)
    }

    @Suppress("unused")
    private val anyLiveRegion = SemanticsMatcher.keyIsDefined(SemanticsProperties.LiveRegion)
}
