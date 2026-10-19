package app.hocket.playback

import app.hocket.core.api.Event
import app.hocket.core.api.TransportState
import app.hocket.core.client.isPositionTick

/**
 * Which core events [PlaybackService] hands to the main thread. Runs on the core's event thread,
 * one event at a time, in order: everything else (downloads' progress, sync progress, lyrics,
 * toasts, search results, ~1 Hz position ticks the session, Connect routes and the watch
 * extrapolate) never wakes the main looper.
 *
 * Keep in step with [PlaybackService.onEvent] and [app.hocket.playback.pebble.PebbleBridge.onEvent].
 */
internal class ServiceEventGate {
    private var lastTransport: TransportState? = null

    fun admit(event: Event): Boolean = when (event) {
        is Event.TransportChanged -> {
            val prev = lastTransport
            lastTransport = event.data.transport
            prev == null || !prev.isPositionTick(event.data.transport)
        }
        is Event.Backend, is Event.MediaSession, is Event.DevicesChanged, is Event.QueueChanged,
        is Event.ServersChanged, is Event.LibraryChanged, is Event.ConnectionChanged, is Event.Started,
        is Event.Snapshot, is Event.SettingChanged, is Event.ResumeOfferChanged -> true
        else -> false
    }
}
