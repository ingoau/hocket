package app.hocket.core

import java.io.IOException

/**
 * Byte-range reads of the core's `hocket-stream://<token>` media sources, for a player that reads
 * through the core (ExoPlayer's `HocketStreamDataSource`). The core fetches, caches and range-serves
 * the audio; the player never sees the server URL or its credentials.
 *
 * Every call BLOCKS the calling thread: use it from ExoPlayer's loader threads, never from the main
 * thread. Failures are [CoreStreamException]s.
 */
interface CoreStreams {
    /** Opens [url] at [offset], returning at most [length] bytes when given. */
    fun streamOpen(url: String, offset: Long, length: Long?): CoreStreamInfo

    /** Up to [maxBytes] from an open stream (the core caps a read at [MAX_READ]); empty = end of input. */
    fun streamRead(handle: Long, maxBytes: Int): ByteArray

    /** Closes a stream; idempotent, never blocks. */
    fun streamClose(handle: Long)

    companion object {
        /** The URL scheme of core streams. */
        const val SCHEME = "hocket-stream"
        /** Largest chunk one read returns (the core's `stream_reader::MAX_READ`). */
        const val MAX_READ = 256 * 1024
    }
}

/** What an open returned; lengths are bytes. */
data class CoreStreamInfo(
    val handle: Long,
    /** Where the first byte read comes from (always the requested offset). */
    val offset: Long,
    /** Size of the whole resource when known (a transcode may not know it). */
    val totalLength: Long?,
    /** Bytes this handle returns before end of input when known. */
    val length: Long?,
    val contentType: String?,
)

/** Why a stream call failed; mirrors the core's `StreamError`. Never carries a URL. */
class CoreStreamException(val kind: Kind, val httpStatus: Int? = null, detail: String? = null) :
    IOException(listOfNotNull(kind.name, httpStatus?.toString(), detail).joinToString(": ")) {
    enum class Kind {
        /** The token is unknown or expired: the core must resolve the item again. */
        UnknownToken,
        UnknownHandle,
        TooManyHandles,
        NoServer,
        RangeNotSatisfiable,
        Status,
        ErrorEnvelope,
        Network,
        Io,
        Closed,
        /** The core has shut down (or was freed): the handle belongs to a dead core. */
        ShutDown,
    }
}
