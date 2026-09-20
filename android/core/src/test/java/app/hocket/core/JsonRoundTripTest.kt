package app.hocket.core

import app.hocket.core.api.*
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.double
import kotlinx.serialization.json.doubleOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Decodes every fixture the Rust side wrote (`cargo test -p hocket-android write_json_fixtures`) with
 * the generated kotlinx types, re-encodes it and compares structurally. Any drift between serde and
 * kotlinx (tagging, naming, unsigned ints, nullable defaults) fails here.
 */
class JsonRoundTripTest {
    private val json = HocketJson.json
    private val fixtures: JsonObject = javaClass.getResourceAsStream("/json-fixtures.json")!!.use {
        json.parseToJsonElement(it.readBytes().decodeToString()).jsonObject
    }

    private fun roundTrip(name: String, decode: (String) -> JsonElement) {
        val original = fixtures[name] ?: error("fixture $name missing")
        val reencoded = decode(original.toString())
        assertEquals("round trip of $name", normalise(original), normalise(reencoded))
    }

    /**
     * Rust writes `Option::None` as an explicit `null`; Kotlin omits it (`explicitNulls = false`) so
     * `#[serde(default)]` fields fall back. Both decode identically, so nulls are dropped before
     * comparing. Numbers are compared by value (`1.7E12` vs `1700000000000.0`).
     */
    private fun normalise(e: JsonElement): JsonElement = when (e) {
        is JsonObject -> JsonObject(e.filterValues { it !is JsonNull }.mapValues { normalise(it.value) })
        is JsonArray -> JsonArray(e.map(::normalise))
        is JsonPrimitive -> if (!e.isString && e.doubleOrNull != null && e.content != "true" && e.content != "false") JsonPrimitive(e.double) else e
        else -> e
    }

    @Test
    fun commandsRoundTrip() {
        fixtures.keys.filter { it.startsWith("command.") }.forEach { name ->
            roundTrip(name) { json.encodeToJsonElement(Command.serializer(), HocketJson.decodeCommand(it)) }
        }
    }

    @Test
    fun queriesRoundTrip() {
        fixtures.keys.filter { it.startsWith("query.") }.forEach { name ->
            roundTrip(name) { json.encodeToJsonElement(Query.serializer(), HocketJson.decodeQuery(it)) }
        }
    }

    @Test
    fun resultsRoundTrip() {
        fixtures.keys.filter { it.startsWith("result.") }.forEach { name ->
            roundTrip(name) { json.encodeToJsonElement(QueryResult.serializer(), HocketJson.decodeQueryResult(it)) }
        }
    }

    @Test
    fun eventsRoundTrip() {
        fixtures.keys.filter { it.startsWith("event.") }.forEach { name ->
            roundTrip(name) { json.encodeToJsonElement(Event.serializer(), HocketJson.decodeEvent(it)) }
        }
    }

    @Test
    fun configRoundTrip() {
        roundTrip("config") { json.encodeToJsonElement(CoreConfig.serializer(), json.decodeFromString(CoreConfig.serializer(), it)) }
    }

    @Test
    fun unitVariantsEncodeAsTypeOnly() {
        assertEquals("""{"type":"play"}""", HocketJson.encodeCommand(Command.Play))
        assertEquals("""{"type":"snapshot"}""", HocketJson.encodeQuery(Query.Snapshot))
    }

    @Test
    fun structVariantsEncodeAdjacently() {
        val text = HocketJson.encodeCommand(Commands.playNext("srv", listOf("a", "b")))
        assertEquals("""{"type":"playNext","data":{"server_id":"srv","track_ids":["a","b"]}}""", text)
    }

    @Test
    fun nullsAreOmittedSoRustDefaultsApply() {
        val text = HocketJson.encodeCommand(Commands.setSleepTimer(null))
        assertEquals("""{"type":"setSleepTimer","data":{}}""", text)
        val item = json.encodeToString(QueueItem.serializer(), QueueItem("k", "t", QueueSource.Inserted, null))
        assertTrue(item, "unavailable" !in item)
    }

    @Test
    fun unknownFieldsAreIgnored() {
        val event = HocketJson.decodeEvent("""{"type":"playerNotice","data":{"message":"x","futureField":1}}""")
        assertTrue(event is Event.PlayerNotice)
        assertEquals("x", (event as Event.PlayerNotice).data.message)
    }

    @Test
    fun toastActionCommandIsDecodable() {
        val toast = (HocketJson.decodeEvent(fixtures["event.toast"].toString()) as Event.Toast).data.toast
        assertEquals(Command.Undo, HocketJson.decodeCommand(toast.actionCommand!!))
        assertEquals("Undo", toast.actionLabel)
        assertEquals(5000u, toast.durationMs)
    }

    @Test
    fun unsignedFieldsDecodeToUInt() {
        val lyrics = (HocketJson.decodeQueryResult(fixtures["result.lyrics"].toString()) as QueryResult.LyricsResult).data!!
        assertEquals(-250, lyrics.offsetMs)
        assertEquals(1500u, lyrics.lines[0].syllables[0].endMs)
        assertEquals("v1", fixtures["result.lyrics"]!!.jsonObject["data"]!!.jsonObject["agents"]!!.let { (it as kotlinx.serialization.json.JsonArray)[0].jsonObject["id"]!!.jsonPrimitive.content })
    }
}
