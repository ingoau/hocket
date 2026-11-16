package app.hocket.core

import app.hocket.core.api.QueueEntry
import app.hocket.core.api.Track
import app.hocket.core.api.TrackRef
import app.hocket.core.api.TrackSummary

/**
 * Songs are named by reference: the account the id is valid in, and the id
 * (`docs/providers.md`, Phase 3). Commands and queries never take a bare track id.
 */
val Track.ref: TrackRef get() = TrackRef(serverId, id)
val TrackSummary.ref: TrackRef get() = TrackRef(serverId, id)
/** The queue entry's own song (the copy it was queued as). */
val QueueEntry.ref: TrackRef get() = item.track

/** Where Mixed playlists live (`Playlist.serverId`), and the New playlist choice for one. */
const val MIXED_SOURCE = "hocket"
