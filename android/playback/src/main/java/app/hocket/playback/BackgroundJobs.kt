package app.hocket.playback

import app.hocket.core.api.Job
import app.hocket.core.api.JobKind
import app.hocket.core.api.JobState
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.transformLatest

/**
 * The core jobs worth keeping the core alive for once the app leaves the screen: downloads, library
 * syncs and playlist imports that are running or about to. A paused job waits on the user and
 * prefetches are best-effort, so neither holds the process.
 *
 * A queued job with [Job.waiting] is held back by the core until the network allows it (offline, or
 * metered with Wi-Fi only downloads) and starts by itself then. It still counts for
 * [WAITING_GRACE_MS] after it last changed: a network blip or a cell flipping metered mid-download
 * must not drop the `dataSync` foreground service, which Android won't let the app start again
 * from the background (the downloads would stall until the app is opened). A longer wait lets go.
 *
 * [PlaybackService] counts these as busy (no idle stop); the app's `JobsService` holds a `dataSync`
 * foreground service with their progress while there are any ([activeFlow], which also notices a
 * grace running out).
 */
object BackgroundJobs {
    private val KINDS = setOf(JobKind.Download, JobKind.LibrarySync, JobKind.PlaylistImport)

    /** How long a job waiting for the network ([Job.waiting]) still counts as active, from its [Job.updatedAt]. */
    const val WAITING_GRACE_MS: Long = 10 * 60 * 1000L

    fun active(jobs: List<Job>, now: Long = System.currentTimeMillis()): List<Job> =
        jobs.filter { it.kind in KINDS && (it.state == JobState.Running || (it.state == JobState.Queued && (it.waiting == null || inGrace(it, now)))) }

    /** When [active] next changes with the clock alone: the earliest end of a grace still running, or null. */
    fun nextGraceEnd(jobs: List<Job>, now: Long = System.currentTimeMillis()): Long? =
        jobs.filter { it.kind in KINDS && it.state == JobState.Queued && it.waiting != null && inGrace(it, now) }
            .minOfOrNull { it.updatedAt.toLong() + WAITING_GRACE_MS }

    /**
     * [active] of each list [jobs] emits, emitted again whenever a waiting job's grace runs out, so a
     * collector lets go once the last one has waited too long even though the core reports nothing new.
     */
    @OptIn(ExperimentalCoroutinesApi::class)
    fun activeFlow(jobs: Flow<List<Job>>, clock: () -> Long = System::currentTimeMillis): Flow<List<Job>> =
        jobs.transformLatest { list ->
            while (true) {
                val now = clock()
                emit(active(list, now))
                val end = nextGraceEnd(list, now) ?: break
                delay((end - now).coerceAtLeast(1L))
            }
        }

    private fun inGrace(job: Job, now: Long): Boolean = now - job.updatedAt.toLong() < WAITING_GRACE_MS

    /** Overall progress of [jobs] as (done, total), or null when any total is not known yet. */
    fun progress(jobs: List<Job>): Pair<Int, Int>? {
        if (jobs.isEmpty() || jobs.any { it.total == null }) return null
        val total = jobs.sumOf { it.total!!.toInt() }
        return if (total <= 0) null else jobs.sumOf { it.done.toInt() }.coerceAtMost(total) to total
    }
}
