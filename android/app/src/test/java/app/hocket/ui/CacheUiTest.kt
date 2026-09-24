package app.hocket.ui

import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.LifecycleRegistry
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.navigation.compose.rememberNavController
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.Commands
import app.hocket.core.SettingKeys
import app.hocket.core.api.NetworkKind
import app.hocket.core.api.NetworkState
import app.hocket.core.api.OfflineState
import app.hocket.core.toSummary
import app.hocket.ui.components.TrackRow
import app.hocket.ui.nav.AppRoot
import app.hocket.ui.queue.QueuePanel
import app.hocket.ui.screens.detail.AlbumDetailScreen
import app.hocket.ui.screens.detail.PRIME_DWELL_MS
import app.hocket.ui.screens.settings.DownloadsSettingsScreen
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/** The stream-cache UI: the album primer, offline badges, the storage settings and "Available offline". */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [35], application = android.app.Application::class, qualifiers = "w411dp-h891dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class CacheUiTest {
    @get:Rule
    val compose = createComposeRule()

    private fun SemanticsNode.state() = config.getOrNull(SemanticsProperties.StateDescription)
    private fun SemanticsNode.desc() = config.getOrNull(SemanticsProperties.ContentDescription)?.joinToString(" ")

    /** The (merged) row whose label starts with [title]. */
    private fun rowOf(title: String): SemanticsNode = compose.onNode(
        SemanticsMatcher("row of $title") { n -> n.config.getOrNull(SemanticsProperties.ContentDescription)?.firstOrNull()?.startsWith("$title,") == true && n.config.contains(SemanticsActions.OnClick) },
    ).fetchSemanticsNode()

    private fun awaitTag(tag: String) {
        compose.waitUntil(5_000) { runCatching { compose.onNodeWithTag(tag).assertExists() }.isSuccess }
        compose.waitForIdle()
    }

    // -- album primer ------------------------------------------------------------------------------

    @Test
    fun theAlbumPageDispatchesPrimeAlbumOnceAfterTwoSecondsOnScreen() {
        val core = TestCore(startPlaying = false)
        core.start()
        val album = core.fake.library.albums[3]
        compose.mainClock.autoAdvance = false
        compose.setThemedContent(core) { AlbumDetailScreen(rememberNavController(), album.id) }
        compose.mainClock.advanceTimeBy(PRIME_DWELL_MS - 300)
        assertEquals("not before the dwell", emptyList<String>(), core.fake.primeRequests)
        compose.mainClock.advanceTimeBy(600)
        assertEquals(listOf("album:${album.id}"), core.fake.primeRequests)
        compose.mainClock.advanceTimeBy(10_000)
        assertEquals("once per visit", listOf("album:${album.id}"), core.fake.primeRequests)
    }

    @Test
    fun leavingTheResumedStateBeforeTheDwellRestartsTheWait() {
        val core = TestCore(startPlaying = false)
        core.start()
        val album = core.fake.library.albums[1]
        val owner = object : LifecycleOwner {
            val registry = LifecycleRegistry.createUnsafe(this).apply { currentState = Lifecycle.State.RESUMED }
            override val lifecycle: Lifecycle get() = registry
        }
        compose.mainClock.autoAdvance = false
        compose.setThemedContent(core) {
            CompositionLocalProvider(LocalLifecycleOwner provides owner) { AlbumDetailScreen(rememberNavController(), album.id) }
        }
        compose.mainClock.advanceTimeBy(1_500)
        compose.runOnUiThread { owner.registry.currentState = Lifecycle.State.STARTED } // another screen / app in the background
        compose.mainClock.advanceTimeBy(5_000)
        assertEquals("not while paused", emptyList<String>(), core.fake.primeRequests)
        compose.runOnUiThread { owner.registry.currentState = Lifecycle.State.RESUMED }
        compose.mainClock.advanceTimeBy(1_500)
        assertEquals("the wait starts over", emptyList<String>(), core.fake.primeRequests)
        compose.mainClock.advanceTimeBy(700)
        assertEquals(listOf("album:${album.id}"), core.fake.primeRequests)
        compose.runOnUiThread { owner.registry.currentState = Lifecycle.State.STARTED }
        compose.runOnUiThread { owner.registry.currentState = Lifecycle.State.RESUMED }
        compose.mainClock.advanceTimeBy(5_000)
        assertEquals("still once for this visit", 1, core.fake.primeRequests.size)
    }

    // -- badges ------------------------------------------------------------------------------------

    @Test
    fun trackRowsSayDownloadedOrCachedInTheirStateWithDistinctBadges() {
        val core = TestCore(startPlaying = false)
        core.start()
        val base = core.fake.library.tracks.first().toSummary()
        val downloaded = base.copy(id = "d", title = "Downloaded one", offline = OfflineState.Downloaded)
        val cached = base.copy(id = "c", title = "Cached one", offline = OfflineState.Cached)
        val none = base.copy(id = "n", title = "Streamed one", offline = OfflineState.None)
        compose.setThemedContent(core) {
            androidx.compose.foundation.layout.Column {
                TrackRow(downloaded, onClick = {}, nowPlaying = true, modifier = Modifier.testTag("row.d"))
                TrackRow(cached, onClick = {}, modifier = Modifier.testTag("row.c"))
                TrackRow(none, onClick = {}, modifier = Modifier.testTag("row.n"))
            }
        }
        val d = compose.onNodeWithTag("row.d").fetchSemanticsNode()
        assertEquals("playing, downloaded", d.state())
        assertEquals("cached, available offline", compose.onNodeWithTag("row.c").fetchSemanticsNode().state())
        assertNull(compose.onNodeWithTag("row.n").fetchSemanticsNode().state())
        // The state is not repeated in the label.
        assertTrue(d.desc()!!.startsWith("Downloaded one,") && "downloaded" !in d.desc()!!)
        // Distinct drawn-only icons; nothing for a track that needs the network.
        compose.onNodeWithTag("offlineBadge.downloaded", useUnmergedTree = true).assertExists()
        compose.onNodeWithTag("offlineBadge.cached", useUnmergedTree = true).assertExists()
        assertNull(compose.onNodeWithTag("offlineBadge.downloaded", useUnmergedTree = true).fetchSemanticsNode().desc())
        assertEquals(2, compose.onAllNodes(SemanticsMatcher("an offline badge") { it.config.getOrNull(SemanticsProperties.TestTag)?.startsWith("offlineBadge.") == true }, useUnmergedTree = true).fetchSemanticsNodes().size)
    }

    @Test
    fun queueItemsCarryTheOfflineState() {
        val core = TestCore()
        // TestCore starts playing the third track of the sixth album.
        val playing = core.fake.library.albumTracks(core.fake.library.albums[5].id)[2]
        core.fake.library.updateTrack(playing.id) { it.copy(offline = OfflineState.Cached) }
        compose.setThemedContent(core) { QueuePanel() }
        core.start()
        compose.waitUntil(5_000) { core.client.queue.value.current?.track?.id == playing.id }
        compose.waitForIdle()
        assertEquals("playing, cached, available offline", rowOf(playing.title).state())
    }

    @Test
    fun albumTrackRowsCarryTheOfflineState() {
        val core = TestCore(startPlaying = false)
        val album = core.fake.library.albums[2]
        val tracks = core.fake.library.albumTracks(album.id)
        core.fake.library.updateTrack(tracks[0].id) { it.copy(offline = OfflineState.Downloaded) }
        core.fake.library.updateTrack(tracks[1].id) { it.copy(offline = OfflineState.Cached) }
        core.start()
        compose.setThemedContent(core) { AlbumDetailScreen(rememberNavController(), album.id) }
        compose.waitUntil(5_000) { runCatching { rowOf(tracks[1].title) }.isSuccess }
        assertEquals("downloaded", rowOf(tracks[0].title).state())
        assertEquals("cached, available offline", rowOf(tracks[1].title).state())
    }

    // -- storage settings --------------------------------------------------------------------------

    @Test
    fun storageSettingsShowCacheUsageBudgetDataSavedAndPrefetch() {
        val core = TestCore(startPlaying = false)
        compose.setThemedContent(core) { DownloadsSettingsScreen(rememberNavController()) }
        core.start()
        compose.waitUntil(5_000) { core.client.storage.value.cacheBudgetBytes != null }
        compose.waitForIdle()
        // 640 MB cache of which 96 MB partial; the automatic budget is 2 GiB.
        rowHas("storage.cacheUsage", "544.0 MB complete · 96.0 MB partial · of 2.15 GB")
        rowHas("storage.cacheMaxBytes", "Automatic (currently 2.15 GB)")
        rowHas("storage.dataSaved", "2.70 GB")
        rowHas("storage.dataSaved", "3.10 GB played from this device · 5.20 GB fetched from the server")

        // A custom size sets storage.cacheMaxBytes; Automatic resets it.
        choice("4 GB").performScrollTo().performClick()
        compose.waitUntil(5_000) { core.client.storage.value.cacheBudgetAuto == false }
        assertEquals(4e9, core.client.settings.value[SettingKeys.STORAGE_CACHE_MAX_BYTES]!!.value.toDouble(), 1.0)
        rowHas("storage.cacheMaxBytes", "Custom: 4.00 GB")
        choice("Automatic").performScrollTo().performClick()
        compose.waitUntil(5_000) { core.client.storage.value.cacheBudgetAuto == true }
        rowHas("storage.cacheMaxBytes", "Automatic (currently 2.15 GB)")

        // Prefetch on mobile data: a device-local switch, off by default.
        val prefetch = compose.onNodeWithTag("setting.storage.prefetchOnMobileData")
        assertEquals(androidx.compose.ui.state.ToggleableState.Off, prefetch.fetchSemanticsNode().config[SemanticsProperties.ToggleableState])
        assertTrue("scope badge read with the title", prefetch.fetchSemanticsNode().config[SemanticsProperties.Text].joinToString(" ") { it.text }.contains("This device"))
        prefetch.performScrollTo().performClick()
        compose.waitUntil(5_000) { core.client.settings.value[SettingKeys.STORAGE_PREFETCH_ON_MOBILE_DATA]?.value == "true" }
    }

    private fun rowHas(tag: String, text: String) =
        compose.onNode(hasAnyAncestor(hasTestTag("setting.$tag")).or(hasTestTag("setting.$tag")).and(hasText(text))).assertExists()

    private fun choice(label: String) = compose.onNode(hasAnyAncestor(hasTestTag("storage.cacheMaxBytes.choices")).and(hasText(label)).and(hasClickAction()))

    // -- available offline -------------------------------------------------------------------------

    @Test
    fun libraryChipAndDownloadsLinkShowOnlyWhatPlaysOffline() {
        val core = TestCore(startPlaying = false)
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        awaitTag("navBar.library")
        compose.onNodeWithTag("navBar.library").performClick()
        compose.onNodeWithText("Songs").performClick()
        awaitTag("library.availableOffline")
        compose.onNodeWithTag("library.availableOffline").performClick()
        compose.waitForIdle()
        assertEquals(true, compose.onNodeWithTag("library.availableOffline").fetchSemanticsNode().config[SemanticsProperties.Selected])
        compose.waitUntil(5_000) { songRows().isNotEmpty() && songRows().size == offlineRows().size }

        compose.onNodeWithTag("library.availableOffline").performClick() // back to all songs
        compose.waitUntil(5_000) { songRows().size > offlineRows().size }
    }

    @Test
    fun downloadsOpensAvailableOffline() {
        val core = TestCore(startPlaying = false)
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        compose.openSettingsFromAccount()
        compose.onNodeWithTag("settings.category.downloads").performScrollTo().performClick()
        compose.onNodeWithTag("setting.open.downloads").performClick()
        awaitTag("downloads.availableOffline")
        compose.onNodeWithTag("downloads.availableOffline").performClick()
        awaitTag("availableOffline.screen")
        compose.waitUntil(5_000) { offlineRows().isNotEmpty() }
        assertEquals("only offline songs", songRows().size, offlineRows().size)
    }

    /** Visible song rows (merged rows with a Play click). */
    private fun songRows(): List<SemanticsNode> = compose.onAllNodes(SemanticsMatcher("a song row") { it.config.getOrNull(SemanticsActions.OnClick)?.label == "Play" && it.config.contains(SemanticsProperties.ContentDescription) }).fetchSemanticsNodes()
    private fun offlineRows() = songRows().filter { n -> n.state()?.let { "downloaded" in it || "cached" in it } == true }

    // -- offline notices ---------------------------------------------------------------------------

    @Test
    fun goingOfflineMidQueueShowsTheNoticeAsASnackbar() {
        val core = TestCore()
        // Some of the queue is not available offline.
        compose.setThemedContent(core) { AppRoot(core.client) }
        core.start()
        awaitTag("navBar.home")
        core.client.dispatch(Commands.setNetworkState(NetworkState(NetworkKind.Offline, false, null)))
        compose.waitUntil(5_000) {
            runCatching { compose.onNodeWithText("Offline: skipping songs that aren't downloaded or cached").assertExists() }.isSuccess ||
                runCatching { compose.onNodeWithText("Nothing in the queue is available offline").assertExists() }.isSuccess
        }
    }
}
