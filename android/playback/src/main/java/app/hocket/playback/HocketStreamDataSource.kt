package app.hocket.playback

import android.content.Context
import android.net.Uri
import androidx.media3.common.C
import androidx.media3.common.PlaybackException
import androidx.media3.datasource.BaseDataSource
import androidx.media3.datasource.DataSource
import androidx.media3.datasource.DataSourceException
import androidx.media3.datasource.DataSpec
import androidx.media3.datasource.DefaultDataSource
import androidx.media3.datasource.DefaultHttpDataSource
import androidx.media3.datasource.TransferListener
import androidx.media3.exoplayer.upstream.DefaultLoadErrorHandlingPolicy
import androidx.media3.exoplayer.upstream.LoadErrorHandlingPolicy
import app.hocket.core.CoreStreamException
import app.hocket.core.CoreStreams
import java.io.IOException

/**
 * Reads `hocket-stream://<token>` media sources through the core ([CoreStreams]) instead of HTTP:
 * the core fetches from the server, fills its evictable stream cache and serves byte ranges from it,
 * so a replay or a seek never goes back to the server and no credential ever reaches the player.
 *
 * - [open]: `streamOpen(uri, position, length)`; returns the handle's length or [C.LENGTH_UNSET].
 *   A seek is ExoPlayer closing this source and opening it again at the new position.
 * - [read]: one blocking FFI call per chunk of [MIN_CHUNK_BYTES] to [CHUNK_BYTES] (the core's 256 KiB
 *   cap); leftover bytes wait in a small buffer for the next [read]; an empty chunk is end of input.
 * - [close]: `streamClose`; idempotent.
 * - An unknown or expired token is a file-not-found [DataSourceException]; [CoreStreamLoadErrorPolicy]
 *   does not retry it, the backend reports a fatal error and the core resolves the item again (fresh
 *   token).
 *
 * Runs on ExoPlayer's loader threads (the calls block). [streams] is read once per [open], so a
 * handle is always closed on the core that opened it. Never wrap this in a `CacheDataSource`: the
 * core already caches.
 */
class HocketStreamDataSource(private val streams: () -> CoreStreams?) : BaseDataSource(/* isNetwork = */ true) {
    private var core: CoreStreams? = null
    private var handle: Long? = null
    private var uri: Uri? = null
    private var opened = false
    /** Bytes still to deliver when the length is known, else [C.LENGTH_UNSET]. */
    private var bytesRemaining: Long = C.LENGTH_UNSET.toLong()
    private var pending: ByteArray = EMPTY
    private var pendingOffset = 0

    override fun open(dataSpec: DataSpec): Long {
        uri = dataSpec.uri
        transferInitializing(dataSpec)
        val s = streams() ?: throw DataSourceException(IOException("the core is not running"), PlaybackException.ERROR_CODE_IO_UNSPECIFIED)
        val length = dataSpec.length.takeIf { it != C.LENGTH_UNSET.toLong() }
        val info = try {
            s.streamOpen(dataSpec.uri.toString(), dataSpec.position, length)
        } catch (e: CoreStreamException) {
            throw toDataSourceException(e)
        }
        core = s
        handle = info.handle
        bytesRemaining = info.length ?: C.LENGTH_UNSET.toLong()
        pending = EMPTY
        pendingOffset = 0
        opened = true
        transferStarted(dataSpec)
        return info.length ?: C.LENGTH_UNSET.toLong()
    }

    override fun read(buffer: ByteArray, offset: Int, length: Int): Int {
        if (length == 0) return 0
        if (bytesRemaining == 0L) return C.RESULT_END_OF_INPUT
        if (pendingOffset >= pending.size) {
            val s = core ?: throw DataSourceException(IOException("stream not open"), PlaybackException.ERROR_CODE_IO_UNSPECIFIED)
            val h = handle ?: throw DataSourceException(IOException("stream not open"), PlaybackException.ERROR_CODE_IO_UNSPECIFIED)
            // At least MIN_CHUNK per call (ExoPlayer's extractors read a few KiB at a time), at most
            // the core's cap, never past the handle's known length.
            var want = length.coerceIn(MIN_CHUNK_BYTES, CHUNK_BYTES)
            if (bytesRemaining > 0) want = minOf(bytesRemaining, want.toLong()).toInt()
            val chunk = try {
                s.streamRead(h, want)
            } catch (e: CoreStreamException) {
                throw toDataSourceException(e)
            }
            if (chunk.isEmpty()) return C.RESULT_END_OF_INPUT
            pending = chunk
            pendingOffset = 0
        }
        val n = minOf(length, pending.size - pendingOffset)
        System.arraycopy(pending, pendingOffset, buffer, offset, n)
        pendingOffset += n
        if (bytesRemaining > 0) bytesRemaining -= n
        bytesTransferred(n)
        return n
    }

    override fun getUri(): Uri? = uri

    override fun close() {
        val h = handle
        val s = core
        handle = null
        core = null
        uri = null
        pending = EMPTY
        pendingOffset = 0
        if (h != null && s != null) s.streamClose(h)
        if (opened) {
            opened = false
            transferEnded()
        }
    }

    class Factory(private val streams: () -> CoreStreams?) : DataSource.Factory {
        override fun createDataSource(): DataSource = HocketStreamDataSource(streams)
    }

    companion object {
        /** The most one FFI read asks for (the core's per-read cap). */
        const val CHUNK_BYTES = CoreStreams.MAX_READ
        /** The least one FFI read asks for, so small extractor reads do not each cross the FFI. */
        const val MIN_CHUNK_BYTES = 64 * 1024
        private val EMPTY = ByteArray(0)

        /**
         * The Media3 error for a core stream failure. [CoreStreamException.Kind.UnknownToken] is
         * file-not-found: `DefaultLoadErrorHandlingPolicy` does not retry it and `ExoBackend` reports
         * it as fatal, which makes the core resolve the item again.
         */
        fun toDataSourceException(e: CoreStreamException): DataSourceException = DataSourceException(e, when (e.kind) {
            CoreStreamException.Kind.UnknownToken -> PlaybackException.ERROR_CODE_IO_FILE_NOT_FOUND
            CoreStreamException.Kind.RangeNotSatisfiable -> PlaybackException.ERROR_CODE_IO_READ_POSITION_OUT_OF_RANGE
            CoreStreamException.Kind.Status, CoreStreamException.Kind.ErrorEnvelope -> PlaybackException.ERROR_CODE_IO_BAD_HTTP_STATUS
            CoreStreamException.Kind.NoServer, CoreStreamException.Kind.Network -> PlaybackException.ERROR_CODE_IO_NETWORK_CONNECTION_FAILED
            else -> PlaybackException.ERROR_CODE_IO_UNSPECIFIED
        })
    }
}

/**
 * Routes by URI scheme when a load opens: `hocket-stream` to [HocketStreamDataSource], everything
 * else (`file:`, `http(s):`, `content:`) to a [DefaultDataSource] over [http]. Transfer listeners
 * are forwarded to both.
 */
class CoreStreamRoutingDataSource(
    private val coreStream: DataSource,
    private val fallback: DataSource,
) : DataSource {
    private var active: DataSource? = null

    override fun addTransferListener(transferListener: TransferListener) {
        coreStream.addTransferListener(transferListener)
        fallback.addTransferListener(transferListener)
    }

    override fun open(dataSpec: DataSpec): Long {
        check(active == null) { "already open" }
        val source = if (isCoreStream(dataSpec.uri)) coreStream else fallback
        active = source
        return source.open(dataSpec)
    }

    override fun read(buffer: ByteArray, offset: Int, length: Int): Int =
        (active ?: throw IOException("not open")).read(buffer, offset, length)

    override fun getUri(): Uri? = active?.uri

    override fun getResponseHeaders(): Map<String, List<String>> = active?.responseHeaders ?: emptyMap()

    override fun close() {
        val source = active ?: return
        active = null
        source.close()
    }

    companion object {
        fun isCoreStream(uri: Uri): Boolean = uri.scheme.equals(CoreStreams.SCHEME, ignoreCase = true)
    }
}

/**
 * The backend's data source factory: core streams through [HocketStreamDataSource] (no
 * `CacheDataSource`, the core caches), everything else through [DefaultDataSource] with an HTTP
 * source carrying the item's [headers]. Stream URLs that still go to the server carry the Subsonic
 * token and salt in the query, so cross-protocol redirects stay off (the Media3 default).
 */
class CoreStreamDataSourceFactory(
    private val context: Context,
    private val streams: () -> CoreStreams?,
    private val headers: Map<String, String> = emptyMap(),
) : DataSource.Factory {
    override fun createDataSource(): DataSource {
        val http = DefaultHttpDataSource.Factory()
            .setUserAgent("Hocket/Android")
            .setConnectTimeoutMs(15_000)
            .setReadTimeoutMs(30_000)
        if (headers.isNotEmpty()) http.setDefaultRequestProperties(headers)
        return CoreStreamRoutingDataSource(HocketStreamDataSource(streams), DefaultDataSource.Factory(context, http).createDataSource())
    }
}

/**
 * `DefaultLoadErrorHandlingPolicy`, except that core stream failures a retry cannot fix fail at once:
 * an unknown/expired token (the core must resolve a fresh one), an offset past the end, and a core
 * that has shut down or closed the handle. Media3's default retries everything but
 * `FileNotFoundException`-typed errors, which would re-open an expired token three times.
 */
class CoreStreamLoadErrorPolicy : DefaultLoadErrorHandlingPolicy() {
    override fun getRetryDelayMsFor(loadErrorInfo: LoadErrorHandlingPolicy.LoadErrorInfo): Long {
        if (coreStreamFailure(loadErrorInfo.exception)?.kind in NOT_RETRYABLE) return C.TIME_UNSET
        return super.getRetryDelayMsFor(loadErrorInfo)
    }

    companion object {
        private val NOT_RETRYABLE = setOf(
            CoreStreamException.Kind.UnknownToken,
            CoreStreamException.Kind.RangeNotSatisfiable,
            CoreStreamException.Kind.ShutDown,
            CoreStreamException.Kind.Closed,
        )

        fun coreStreamFailure(e: Throwable?): CoreStreamException? {
            var t = e
            while (t != null) {
                if (t is CoreStreamException) return t
                t = t.cause
            }
            return null
        }
    }
}
