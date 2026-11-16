package app.hocket.playback

import app.hocket.core.api.Command
import app.hocket.core.api.YtmJsRequest
import app.hocket.core.api.YtmJsRequestDecipherInner
import app.hocket.core.api.YtmJsRequestPrepareInner
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

private const val SCRIPT = "/* host script */"

/** A page whose `hocketYtm` is faked: it answers only once the host script is loaded. */
private class FakePage : JsPage {
    var scripts = 0
    val calls = mutableListOf<String>()
    override var closed = false
    var failNext: String? = null
    /** The next call waits for this before answering. */
    var hold: CompletableDeferred<Unit>? = null

    override suspend fun load(script: String) {
        check(script == SCRIPT)
        scripts++
    }

    override suspend fun run(code: String): String {
        hold?.let { hold = null; it.await() }
        check(!closed) { "the page is gone" }
        check(scripts > 0) { "hocketYtm is not defined" }
        calls += code
        failNext?.let { failNext = null; error(it) }
        Regex("""^hocketYtm\.prepare\("(.*)", "(.*)"\)$""").find(code)?.let {
            return """{"signatureTimestamp":20767,"poToken":"pot-for-${it.groupValues[2]}","extra":1}"""
        }
        Regex("""^hocketYtm\.decipher\("(.*?)", (\[.*])\)$""").find(code)?.let { m ->
            val formats = Json.parseToJsonElement(m.groupValues[2]) as JsonArray
            val urls = formats.mapIndexed { i, f -> "\"https://rr1.googlevideo.com/videoplayback?signed=$i&len=${f.jsonPrimitive.content.length}\"" }
            return """{"urls":[${urls.joinToString(",")}]}"""
        }
        error("unexpected $code")
    }

    override fun close() {
        closed = true
    }
}

@OptIn(ExperimentalCoroutinesApi::class)
class YtmJsHostTest {
    private val prepare = YtmJsRequest.Prepare(YtmJsRequestPrepareInner("4D7u5KF7SP8", "CgtWaXNpdG9y"))
    private val decipher = YtmJsRequest.Decipher(
        YtmJsRequestDecipherInner("4D7u5KF7SP8", listOf("s=ABC&sp=sig&url=https%3A%2F%2Frr1.googlevideo.com%2Fvideoplayback", "https://rr1.googlevideo.com/videoplayback?n=xyz")),
    )

    private fun answers(sent: List<Command>) = sent.filterIsInstance<Command.YtmJsAnswer>().map { it.data }

    @Test
    fun loadsTheScriptOnceThenAnswersEachRequest() = runTest {
        val pages = mutableListOf<FakePage>()
        val sent = mutableListOf<Command>()
        val host = YtmJsHost(backgroundScope, { FakePage().also { pages += it } }, { SCRIPT }, { sent += it })
        host.handle("r1", prepare)
        host.handle("r2", decipher)
        assertEquals(1, pages.size)
        assertEquals(1, pages[0].scripts)
        val (a1, a2) = answers(sent)
        assertEquals("r1", a1.request_id)
        assertEquals(20767u, a1.answer?.signatureTimestamp)
        assertEquals("pot-for-CgtWaXNpdG9y", a1.answer?.poToken)
        assertNull(a1.error)
        assertEquals("r2", a2.request_id)
        assertEquals(2, a2.answer?.urls?.size)
        assertTrue(a2.answer!!.urls!![0].contains("signed=0"))
        host.close()
    }

    @Test
    fun aFailureIsAnErrorAnswerAndADeadPageIsMadeAgain() = runTest {
        val pages = mutableListOf<FakePage>()
        val sent = mutableListOf<Command>()
        val host = YtmJsHost(backgroundScope, { FakePage().also { pages += it } }, { SCRIPT }, { sent += it })
        host.handle("r1", prepare)
        pages[0].failNext = "BotGuard: no VM"
        host.handle("r2", prepare)
        assertEquals("BotGuard: no VM", answers(sent)[1].error)
        assertNull(answers(sent)[1].answer)
        host.handle("r3", prepare)
        assertEquals(1, pages.size)
        pages[0].close()
        host.handle("r4", prepare)
        assertEquals(2, pages.size)
        assertEquals(1, pages[1].scripts)
        assertEquals("pot-for-CgtWaXNpdG9y", answers(sent)[3].answer?.poToken)
        host.close()
    }

    @Test
    fun closesThePageOnceIdle() = runTest {
        val pages = mutableListOf<FakePage>()
        val host = YtmJsHost(backgroundScope, { FakePage().also { pages += it } }, { SCRIPT }, { }, idleMs = 1_000)
        host.handle("r1", prepare)
        assertFalse(pages[0].closed)
        advanceTimeBy(1_500)
        runCurrent()
        assertTrue(pages[0].closed)
    }

    @Test
    fun theIdleTimerWaitsForEveryRequestInFlight() = runTest {
        val pages = mutableListOf<FakePage>()
        val sent = mutableListOf<Command>()
        val host = YtmJsHost(backgroundScope, { FakePage().also { pages += it } }, { SCRIPT }, { sent += it }, idleMs = 1_000)
        host.handle("r0", prepare)
        val gate = CompletableDeferred<Unit>()
        pages[0].hold = gate
        val slow = launch { host.handle("r1", prepare) }
        runCurrent()
        // r2 finishes while r1 is still running: no timer may close the page under r1.
        host.handle("r2", prepare)
        advanceTimeBy(1_500)
        runCurrent()
        assertFalse(pages[0].closed)
        gate.complete(Unit)
        slow.join()
        assertNull(answers(sent).single { it.request_id == "r1" }.error)
        assertFalse(pages[0].closed)
        advanceTimeBy(1_500)
        runCurrent()
        assertTrue("idle once the last request is answered", pages[0].closed)
    }

    @Test
    fun buildsTheCallsAndRefusesAShortAnswer() {
        assertEquals("hocketYtm.prepare(\"4D7u5KF7SP8\", \"CgtWaXNpdG9y\")", YtmJsHost.callFor(prepare))
        assertTrue(YtmJsHost.callFor(decipher).startsWith("hocketYtm.decipher(\"4D7u5KF7SP8\", [\"s=ABC"))
        assertTrue(runCatching { YtmJsHost.answerOf(decipher, """{"urls":["one"]}""") }.isFailure)
        val a = YtmJsHost.answerOf(prepare, """{"signatureTimestamp":"x","poToken":""}""")
        assertNull(a.signatureTimestamp)
        assertNull(a.poToken)
    }
}
