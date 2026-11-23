package app.hocket.playback

import app.hocket.core.api.Job
import app.hocket.core.api.JobKind
import app.hocket.core.api.JobState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** Which core jobs keep the core alive (no idle stop, the jobs foreground service). */
class BackgroundJobsTest {
    private fun job(kind: JobKind, state: JobState, done: Int = 0, total: Int? = null) =
        Job("$kind-$state", kind, "label", state, done.toUInt(), total?.toUInt(), 0u, 0.0, 0.0, true)

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
    fun progressSumsKnownTotals() {
        assertEquals(5 to 30, BackgroundJobs.progress(listOf(job(JobKind.Download, JobState.Running, 3, 10), job(JobKind.Download, JobState.Running, 2, 20))))
        assertNull(BackgroundJobs.progress(listOf(job(JobKind.Download, JobState.Running, 3, 10), job(JobKind.LibrarySync, JobState.Running, 2))))
        assertNull(BackgroundJobs.progress(emptyList()))
    }
}
