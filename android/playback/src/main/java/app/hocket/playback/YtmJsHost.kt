package app.hocket.playback

import app.hocket.core.api.Command
import app.hocket.core.api.CommandYtmJsAnswerInner
import app.hocket.core.api.YtmJsAnswer
import app.hocket.core.api.YtmJsRequest
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonPrimitive

/** A page that runs JavaScript: the hidden WebView ([WebViewJsPage]), a fake in tests. */
interface JsPage {
    /** Run [script] (the host script) once. */
    suspend fun load(script: String)

    /** Evaluate the expression [code], awaiting a promise it yields; its value as JSON. */
    suspend fun run(code: String): String

    val closed: Boolean

    fun close()
}

/**
 * YouTube Music's JS host (docs/providers.md, Phase 4 "Premium JS path"): a hidden page on
 * https://www.youtube.com runs the core's host script (yt-dlp's EJS solver plus `hocketYtm`, from
 * `Query.YtmJsHostScript`) and answers the core's `YtmJsRequest` events with `YtmJsAnswer`: the
 * player's signature timestamp and a PO token, or signed, `n`-transformed stream URLs. The page is
 * made on the first request and closed after [idleMs] without one.
 */
class YtmJsHost(
    private val scope: CoroutineScope,
    private val open: suspend () -> JsPage,
    private val script: suspend () -> String,
    private val dispatch: (Command) -> Unit,
    private val idleMs: Long = IDLE_CLOSE_MS,
) {
    private val lock = Mutex()
    private var page: JsPage? = null
    private var idle: Job? = null

    /** Answer one request; never throws (a failure is the answer's `error`). */
    suspend fun handle(requestId: String, request: YtmJsRequest) {
        idle?.cancel()
        val result = runCatching {
            val p = ready()
            answerOf(request, p.run(callFor(request)))
        }
        val e = result.exceptionOrNull()
        if (e is kotlinx.coroutines.CancellationException) throw e
        if (e != null) lock.withLock { if (page?.closed == true) page = null }
        dispatch(Command.YtmJsAnswer(CommandYtmJsAnswerInner(requestId, result.getOrNull(), e?.let { it.message ?: it.javaClass.simpleName })))
        idle = scope.launch {
            delay(idleMs)
            close()
        }
    }

    suspend fun close() {
        idle?.cancel()
        lock.withLock {
            page?.close()
            page = null
        }
    }

    private suspend fun ready(): JsPage = lock.withLock {
        page?.takeUnless { it.closed }?.let { return it }
        val p = open()
        try {
            p.load(script())
        } catch (e: Exception) {
            p.close()
            throw e
        }
        page = p
        p
    }

    companion object {
        const val IDLE_CLOSE_MS = 10 * 60_000L

        private fun str(s: String) = JsonPrimitive(s).toString()

        /** The expression that answers [request] once the host script is in the page. */
        fun callFor(request: YtmJsRequest): String = when (request) {
            is YtmJsRequest.Prepare -> "hocketYtm.prepare(${str(request.data.video_id)}, ${str(request.data.visitor_data)})"
            is YtmJsRequest.Decipher ->
                "hocketYtm.decipher(${str(request.data.video_id)}, ${JsonArray(request.data.formats.map(::JsonPrimitive))})"
        }

        /** The page's result (JSON) as the core's answer; unknown fields dropped, wrong types refused. */
        fun answerOf(request: YtmJsRequest, json: String): YtmJsAnswer {
            val v = runCatching { Json.parseToJsonElement(json) }.getOrNull() as? JsonObject ?: JsonObject(emptyMap())
            return when (request) {
                is YtmJsRequest.Decipher -> {
                    val urls = (v["urls"] as? JsonArray)?.mapNotNull { (it as? JsonPrimitive)?.takeIf { p -> p.isString }?.content }
                    if (urls == null || urls.size != request.data.formats.size) error("the JS host signed no URLs")
                    YtmJsAnswer(urls = urls)
                }
                is YtmJsRequest.Prepare -> YtmJsAnswer(
                    signatureTimestamp = (v["signatureTimestamp"] as? JsonPrimitive)?.intOrNull?.takeIf { it > 0 }?.toUInt(),
                    poToken = (v["poToken"] as? JsonPrimitive)?.takeIf { it.isString }?.contentOrNull?.takeIf { it.isNotEmpty() },
                )
            }
        }
    }
}
