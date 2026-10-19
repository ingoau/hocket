package app.hocket.playback

import android.app.Application
import android.os.Looper
import androidx.media3.common.MediaItem
import androidx.media3.common.Player
import androidx.media3.common.Timeline
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.hocket.core.api.Command
import app.hocket.core.api.MediaSessionAction
import app.hocket.core.api.MediaSessionMetadata
import app.hocket.core.api.MediaSessionState
import app.hocket.core.api.OfflineState
import app.hocket.core.api.PositionStamp
import app.hocket.core.api.QueueEntry
import app.hocket.core.api.QueueItem
import app.hocket.core.api.QueueMode
import app.hocket.core.api.QueueSource
import app.hocket.core.api.QueueView
import app.hocket.core.api.RepeatMode
import app.hocket.core.api.TrackSummary
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

/**
 * The session's playlist is the core's queue: controllers see history, the current entry and what
 * comes next, and picking, moving, removing or adding entries sends the queue commands.
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [34], application = Application::class)
class CoreSessionQueueTest {
    private val sent = mutableListOf<Command>()
    private val requests = mutableListOf<Pair<String, List<String>>>()
    private val media = object : CoreSessionPlayer.MediaRequests {
        override fun play(mediaIds: List<String>, startIndex: Int) { requests += "play@$startIndex" to mediaIds }
        override fun enqueue(mediaIds: List<String>, next: Boolean) { requests += (if (next) "next" else "later") to mediaIds }
    }
    private val player = CoreSessionPlayer(Looper.getMainLooper(), { sent += it }, media = media)

    private fun entry(n: Int) = QueueEntry(
        QueueItem("k$n", "t$n", QueueSource.Inserted, false),
        TrackSummary("t$n", "s1", "Track $n", "Artist", "Album", "al1", "ar1", 180_000u, null, 0u, false, OfflineState.None),
    )

    private fun state(trackId: String) = MediaSessionState(
        MediaSessionMetadata("Track now", "Artist", "Album", 200_000u, null, trackId, false, 0u), true,
        PositionStamp(0u, 0.0, 1.0, true), false, RepeatMode.Off, 1.0, false,
        listOf(MediaSessionAction.Play, MediaSessionAction.Pause, MediaSessionAction.Next, MediaSessionAction.Previous, MediaSessionAction.Seek), true,
    )

    /** History k0,k1; current k2; playing next k3; upcoming k4,k5. */
    private val queue = QueueView("Album", listOf(entry(0), entry(1)), entry(2), listOf(entry(3)), listOf(entry(4), entry(5)), false, RepeatMode.Off, false, QueueMode.Apple, 2u)

    private fun settle() = shadowOf(Looper.getMainLooper()).idle()

    private fun setUp() {
        player.apply(state("t2"))
        player.applyQueue(queue)
        settle()
    }

    @Test
    fun positionOnlyApplyDoesNotRebuildTheTimeline() {
        setUp()
        var timelineChanges = 0
        player.addListener(object : Player.Listener {
            override fun onTimelineChanged(timeline: Timeline, reason: Int) {
                timelineChanges++
            }
        })
        settle()
        val start = timelineChanges
        val base = state("t2")
        repeat(40) { i ->
            player.apply(base.copy(position = PositionStamp((i * 750).toUInt(), i.toDouble(), 1.0, true)))
            settle()
        }
        assertEquals("40 position ticks must not rebuild the Media3 timeline", start, timelineChanges)
        player.apply(base.copy(metadata = base.metadata!!.copy(title = "Other")))
        settle()
        assertTrue("a metadata change still rebuilds", timelineChanges > start)
    }

    @Test
    fun applyQueueWithTheSameKeysDoesNotRebuildTheTimeline() {
        setUp()
        var timelineChanges = 0
        player.addListener(object : Player.Listener {
            override fun onTimelineChanged(timeline: Timeline, reason: Int) {
                timelineChanges++
            }
        })
        settle()
        val start = timelineChanges
        player.applyQueue(queue)
        settle()
        val loved = queue.current!!.track.copy(loved = true)
        player.applyQueue(queue.copy(current = queue.current!!.copy(track = loved)))
        settle()
        assertEquals("metadata-only queue views must not rebuild the playlist", start, timelineChanges)
        player.applyQueue(queue.copy(upcoming = listOf(entry(9))))
        settle()
        assertTrue("a membership change still rebuilds", timelineChanges > start)
    }

    @Test
    fun thePlaylistIsTheQueueAroundTheCurrentEntry() {
        setUp()
        assertEquals(6, player.mediaItemCount)
        assertEquals(2, player.currentMediaItemIndex)
        assertEquals("Track now", player.currentMediaItem?.mediaMetadata?.title.toString())
        assertEquals("Track 4", player.getMediaItemAt(4).mediaMetadata.title.toString())
        assertEquals(MediaId.QueueItem("k5").format(), player.getMediaItemAt(5).mediaId)
    }

    @Test
    fun duplicateQueueKeysAreDroppedInsteadOfCrashing() {
        player.apply(state("t2"))
        // k2 (current) repeated in history and upcoming, k3 repeated in upcoming (#54).
        player.applyQueue(queue.copy(history = listOf(entry(0), entry(2)), upcoming = listOf(entry(3), entry(4), entry(2))))
        settle()
        assertEquals(4, player.mediaItemCount)
        assertEquals(1, player.currentMediaItemIndex)
        assertEquals(MediaId.QueueItem("k4").format(), player.getMediaItemAt(3).mediaId)
    }

    @Test
    fun aQueueThatHasNotCaughtUpShowsOnlyTheCurrentTrack() {
        player.apply(state("t9"))
        player.applyQueue(queue)
        settle()
        assertEquals(1, player.mediaItemCount)
        assertEquals("t9", player.currentMediaItem?.mediaId)
    }

    @Test
    fun historyIsCapped() {
        val long = queue.copy(history = (100 until 160).map(::entry))
        assertEquals(CoreSessionPlayer.HISTORY_SHOWN, CoreSessionPlayer.timeline(long, "t2").currentIndex)
        assertEquals("k159", CoreSessionPlayer.timeline(long, "t2").entries[CoreSessionPlayer.HISTORY_SHOWN - 1].item.key)
    }

    @Test
    fun upcomingIsCapped() {
        val long = queue.copy(upcoming = (100 until 400).map(::entry))
        val t = CoreSessionPlayer.timeline(long, "t2")
        assertEquals(t.currentIndex + 1 + CoreSessionPlayer.UPCOMING_SHOWN, t.entries.size)
        assertEquals("k3", t.entries[t.currentIndex + 1].item.key)
    }

    @Test
    fun pickingAnEntryJumpsToIt() {
        setUp()
        player.seekTo(4, 0)
        assertEquals("k4", (sent.single() as Command.JumpToQueueItem).data.key)
        sent.clear()
        player.seekTo(0, 0)
        assertEquals("k0", (sent.single() as Command.JumpToQueueItem).data.key)
    }

    @Test
    fun nextAndPreviousStayTheCoresEvenWithAPlaylist() {
        setUp()
        player.seekToNext()
        assertEquals(MediaSessionAction.Next, (sent.single() as Command.MediaSessionCommand).data.action)
    }

    @Test
    fun movingAnUpcomingEntryUsesTheCombinedUpcomingIndex() {
        setUp()
        player.moveMediaItem(5, 3)
        val move = sent.single() as Command.MoveQueueItem
        assertEquals("k5", move.data.key)
        assertEquals(0u, move.data.to_index)
        sent.clear()
        setUp()
        player.moveMediaItem(3, 1)
        assertTrue("history and the current entry are not reorderable", sent.isEmpty())
    }

    @Test
    fun removingSkipsTheCurrentEntry() {
        setUp()
        player.removeMediaItems(2, 5)
        assertEquals(listOf("k3", "k4"), (sent.single() as Command.RemoveQueueItems).data.keys)
    }

    @Test
    fun addedAndSetItemsGoToTheLibrary() {
        setUp()
        player.addMediaItem(3, MediaItem.Builder().setMediaId("track/s1/t9").build())
        assertEquals("next" to listOf("track/s1/t9"), requests.last())
        player.addMediaItem(MediaItem.Builder().setMediaId("track/s1/t8").build())
        assertEquals("later" to listOf("track/s1/t8"), requests.last())
        player.setMediaItems(listOf(MediaItem.Builder().setMediaId("album/s1/al1").build()), 0, 0)
        assertEquals("play@0" to listOf("album/s1/al1"), requests.last())
    }
}
