package app.hocket.core

import app.hocket.core.api.Command
import app.hocket.core.api.Event
import app.hocket.core.api.Query
import app.hocket.core.api.QueryResult
import kotlinx.serialization.json.Json

/**
 * The one JSON configuration used for everything that crosses the core seam.
 *
 * Matches the Rust side exactly: adjacently tagged enums (`{"type":"playNext","data":{...}}`,
 * unit variants `{"type":"play"}`), unknown fields ignored so an older app survives a newer core,
 * defaults encoded so the Rust side never sees a missing required field, and nulls omitted so
 * `#[serde(default)]` fields (typeshare makes them nullable) fall back to their Rust default.
 */
object HocketJson {
    val json: Json = Json {
        classDiscriminator = "type"
        ignoreUnknownKeys = true
        encodeDefaults = true
        explicitNulls = false
    }

    fun encodeCommand(command: Command): String = json.encodeToString(Command.serializer(), command)
    fun decodeCommand(text: String): Command = json.decodeFromString(Command.serializer(), text)
    fun encodeQuery(query: Query): String = json.encodeToString(Query.serializer(), query)
    fun decodeQuery(text: String): Query = json.decodeFromString(Query.serializer(), text)
    fun decodeQueryResult(text: String): QueryResult = json.decodeFromString(QueryResult.serializer(), text)
    fun encodeQueryResult(result: QueryResult): String = json.encodeToString(QueryResult.serializer(), result)
    fun decodeEvent(text: String): Event = json.decodeFromString(Event.serializer(), text)
    fun encodeEvent(event: Event): String = json.encodeToString(Event.serializer(), event)
}
