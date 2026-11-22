package app.hocket.playback

import app.hocket.core.api.Job
import app.hocket.core.api.JobKind
import app.hocket.core.api.JobState

/**
 * The core jobs worth keeping the core alive for once the app leaves the screen: downloads, library
 * syncs and playlist imports that are running or about to. A paused job waits on something else
 * (the network, the user) and prefetches are best-effort, so neither holds the process.
 *
 * [PlaybackService] counts these as busy (no idle stop); the app's `JobsService` holds a `dataSync`
 * foreground service with their progress while there are any.
 */
object BackgroundJobs {
    private val KINDS = setOf(JobKind.Download, JobKind.LibrarySync, JobKind.PlaylistImport)

    fun active(jobs: List<Job>): List<Job> =
        jobs.filter { it.kind in KINDS && (it.state == JobState.Running || it.state == JobState.Queued) }

    /** Overall progress of [jobs] as (done, total), or null when any total is not known yet. */
    fun progress(jobs: List<Job>): Pair<Int, Int>? {
        if (jobs.isEmpty() || jobs.any { it.total == null }) return null
        val total = jobs.sumOf { it.total!!.toInt() }
        return if (total <= 0) null else jobs.sumOf { it.done.toInt() }.coerceAtMost(total) to total
    }
}
