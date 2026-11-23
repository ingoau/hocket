package app.hocket.playback

import app.hocket.core.api.Job
import app.hocket.core.api.JobKind
import app.hocket.core.api.JobState
import app.hocket.core.api.JobWait
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.currentTime
import kotlinx.coroutines.test.runTest
import org.junit.Test

/** Which core jobs keep the core alive (no idle stop, the jobs foreground service). */
class BackgroundJobsTest {
    private fun job(kind: JobKind, state: JobState, done: Int = 0, total: Int? = null, waiting: JobWait? = null, updatedAt: Long = 0L) =
        Job("$kind-$state-$waiting-$updatedAt", kind, "label", state, done.toUInt(), total?.toUInt(), 0u, 0.0, updatedAt.toDouble(), true, waiting)

    private val grace = BackgroundJobs.WAITING_GRACE_MS

    @Test
    fun runningAndQueuedDownloadsAndSyncsCount() {
        val jobs = listOf(
            job(JobKind.Download, JobState.Running), job(JobKind.LibrarySync, JobState.Queued),
            job(JobKind.PlaylistImport, JobState.Running),
        )
        assertEquals(jobs, BackgroundJobs.active(jobs))
    }

    @Test
    fun finishedPausedAndPrefetchJobsDoNot() {
        val jobs = listOf(
            job(JobKind.Download, JobState.Done), job(JobKind.Download, JobState.Failed),
            job(JobKind.Download, JobState.Cancelled), job(JobKind.Download, JobState.Paused),
            job(JobKind.ArtworkPrefetch, JobState.Running), job(JobKind.LyricsPrefetch, JobState.Running),
        )
        assertTrue(BackgroundJobs.active(jobs).isEmpty())
    }

    @Test
    fun queuedJobsWaitingForTheNetworkCountOnlyForTheGrace() {
        // Held for Wi-Fi or a connection: a short blip keeps the foreground service, a long wait lets go.
        val now = 100 * grace
        val waiting = listOf(
            job(JobKind.Download, JobState.Queued, waiting = JobWait.Metered, updatedAt = now - grace + 1),
            job(JobKind.LibrarySync, JobState.Queued, waiting = JobWait.Offline, updatedAt = now - 1_000),
            job(JobKind.PlaylistImport, JobState.Queued, waiting = JobWait.Offline, updatedAt = now),
        )
        assertEquals(waiting, BackgroundJobs.active(waiting, now))
        val stale = listOf(
            job(JobKind.Download, JobState.Queued, waiting = JobWait.Metered, updatedAt = now - grace),
            job(JobKind.LibrarySync, JobState.Queued, waiting = JobWait.Offline, updatedAt = now - 5 * grace),
        )
        assertTrue(BackgroundJobs.active(stale, now).isEmpty())
        val ready = job(JobKind.Download, JobState.Queued)
        assertEquals(listOf(ready), BackgroundJobs.active(stale + ready, now))
        // A waiting prefetch never counted in the first place.
        assertTrue(BackgroundJobs.active(listOf(job(JobKind.ArtworkPrefetch, JobState.Queued, waiting = JobWait.Offline, updatedAt = now)), now).isEmpty())
    }

    @Test
    fun nextGraceEndIsTheEarliestRunningGrace() {
        val now = 100 * grace
        val jobs = listOf(
            job(JobKind.Download, JobState.Queued, waiting = JobWait.Offline, updatedAt = now - 2_000),
            job(JobKind.Download, JobState.Queued, waiting = JobWait.Metered, updatedAt = now - 5_000),
            job(JobKind.Download, JobState.Queued, waiting = JobWait.Metered, updatedAt = now - 2 * grace),
            job(JobKind.Download, JobState.Running),
        )
        assertEquals(now - 5_000 + grace, BackgroundJobs.nextGraceEnd(jobs, now))
        assertNull(BackgroundJobs.nextGraceEnd(listOf(job(JobKind.Download, JobState.Running)), now))
    }

    @Test
    fun activeFlowLetsGoWhenTheGraceRunsOut() = runTest {
        val start = currentTime
        val waiting = job(JobKind.Download, JobState.Queued, waiting = JobWait.Offline, updatedAt = start)
        val seen = mutableListOf<Pair<Long, Int>>()
        val collector = launch {
            BackgroundJobs.activeFlow(flowOf(listOf(waiting)), clock = { currentTime }).collect { seen += (currentTime - start) to it.size }
        }
        advanceUntilIdle()
        collector.join()
        // Active at first, then nothing once the grace has passed, without the core reporting anything new.
        assertEquals(listOf(0L to 1, grace to 0), seen)
    }

    @Test
    fun progressSumsKnownTotals() {
        assertEquals(5 to 30, BackgroundJobs.progress(listOf(job(JobKind.Download, JobState.Running, 3, 10), job(JobKind.Download, JobState.Running, 2, 20))))
        assertNull(BackgroundJobs.progress(listOf(job(JobKind.Download, JobState.Running, 3, 10), job(JobKind.LibrarySync, JobState.Running, 2))))
        assertNull(BackgroundJobs.progress(emptyList()))
    }
}
