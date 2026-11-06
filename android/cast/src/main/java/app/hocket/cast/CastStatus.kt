package app.hocket.cast

import com.google.android.gms.cast.MediaStatus

/**
 * What a receiver status means for the core's reports, per item: each change is reported once
 * (statuses repeat), `Ready` once per load, `Ended` once per item. Pure, so it is unit-tested.
 */
internal class CastStatus {
    enum class Event { Ready, Playing, Paused, Buffering, Ended, Failed }

    private enum class Last { None, Playing, Paused, Buffering, Ended, Failed }

    private var key: String? = null
    private var ready = false
    private var last = Last.None

    fun reset() {
        key = null
        ready = false
        last = Last.None
    }

    /** A load of [key] was sent: it reports `Ready` again. */
    fun loading(key: String) {
        this.key = key
        ready = false
        last = Last.Buffering
    }

    /** The receiver moved on to [key] by itself (it was preloaded: no `Ready` to wait for). */
    fun transitioned(key: String) {
        this.key = key
        ready = true
        last = Last.None
    }

    fun failed() {
        last = Last.Failed
    }

    fun update(key: String, playerState: Int, idleReason: Int): List<Event> {
        if (this.key != key) loading(key)
        val out = mutableListOf<Event>()
        fun readyFirst() {
            if (!ready) {
                ready = true
                out += Event.Ready
            }
        }
        when (playerState) {
            MediaStatus.PLAYER_STATE_PLAYING -> {
                readyFirst()
                if (last != Last.Playing) out += Event.Playing
                last = Last.Playing
            }
            MediaStatus.PLAYER_STATE_PAUSED -> {
                readyFirst()
                if (last != Last.Paused) out += Event.Paused
                last = Last.Paused
            }
            MediaStatus.PLAYER_STATE_BUFFERING, MediaStatus.PLAYER_STATE_LOADING -> {
                if (last != Last.Buffering) out += Event.Buffering
                last = Last.Buffering
            }
            MediaStatus.PLAYER_STATE_IDLE -> when (idleReason) {
                MediaStatus.IDLE_REASON_FINISHED -> {
                    if (last != Last.Ended) out += Event.Ended
                    last = Last.Ended
                }
                MediaStatus.IDLE_REASON_ERROR -> {
                    if (last != Last.Failed) out += Event.Failed
                    last = Last.Failed
                }
                // Stopped or replaced by a load: nothing to say about this item.
                else -> Unit
            }
        }
        return out
    }
}
