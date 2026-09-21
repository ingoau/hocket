package app.hocket.realcore

import java.io.BufferedReader
import java.io.ByteArrayOutputStream
import java.io.InputStreamReader
import java.net.InetAddress
import java.net.ServerSocket
import java.net.Socket
import java.net.URI
import java.net.URLDecoder
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.Executors
import kotlin.concurrent.thread

/**
 * An in-process Navidrome 0.63 stand-in over plain HTTP, serving the core's own Subsonic fixtures
 * (`crates/hocket-core/src/subsonic/fixtures`) so the real core can be driven end-to-end on the JVM.
 *
 * Endpoints: ping, getOpenSubsonicExtensions, getMusicFolders, getArtists/getArtist, getAlbumList2
 * (paged: only offset 0 has content), getAlbum, getSong, search3 (paged the same way), getPlaylists,
 * getPlaylist, getGenres, getScanStatus, getCoverArt (a PNG), stream (a short WAV),
 * getLyricsBySongId, setRating/star/unstar/scrobble (accepted, recorded), getSimilarSongs2/getTopSongs
 * (empty), and a 404 on /auth/login (no native API). Every request is recorded in [calls]; a wrong
 * password gets the Subsonic error 40.
 */
class FakeNavidrome(private val expectedUser: String = "alice", private val expectedPassword: String = "secret") {
    val calls = CopyOnWriteArrayList<String>()
    val ratings = HashMap<String, Int>()
    val starred = HashSet<String>()
    val scrobbles = CopyOnWriteArrayList<Pair<String, Boolean>>()
    // A minimal HTTP/1.1 server on ServerSocket: unit tests compile against android.jar, which has no
    // com.sun.net.httpserver.
    private val server = ServerSocket(0, 50, InetAddress.getLoopbackAddress())
    private val pool = Executors.newCachedThreadPool()
    @Volatile private var running = false
    val baseUrl: String get() = "http://127.0.0.1:${server.localPort}"

    /** One parsed request. */
    class HttpExchange(val method: String, val requestURI: URI, val headers: Map<String, String>, private val socket: Socket) {
        val responseHeaders = HashMap<String, String>()
        fun respond(status: Int, body: ByteArray, type: String) {
            val reason = when (status) { 200 -> "OK"; 404 -> "Not Found"; 500 -> "Internal Server Error"; else -> "Status" }
            val head = StringBuilder("HTTP/1.1 $status $reason\r\n")
            head.append("Content-Type: $type\r\n").append("Content-Length: ${body.size}\r\n").append("Connection: close\r\n")
            responseHeaders.forEach { (k, v) -> head.append("$k: $v\r\n") }
            head.append("\r\n")
            socket.getOutputStream().use { out -> out.write(head.toString().toByteArray()); out.write(body); out.flush() }
        }
    }

    private fun fixture(name: String): ByteArray =
        javaClass.getResourceAsStream("/navidrome/$name.json")?.use { it.readBytes() } ?: error("missing fixture $name")

    private val ok = """{"subsonic-response":{"status":"ok","version":"1.16.1","type":"navidrome","serverVersion":"0.63.1 (abcd1234)","openSubsonic":true}}"""
    private val authError = """{"subsonic-response":{"status":"failed","version":"1.16.1","type":"navidrome","serverVersion":"0.63.1 (abcd1234)","openSubsonic":true,"error":{"code":40,"message":"Wrong username or password"}}}"""

    fun start() {
        running = true
        thread(name = "fake-navidrome", isDaemon = true) {
            while (running) {
                val socket = try { server.accept() } catch (e: Exception) { break }
                pool.execute { try { serve(socket) } catch (e: Exception) { runCatching { socket.close() } } }
            }
        }
    }

    fun stop() {
        running = false
        runCatching { server.close() }
        pool.shutdownNow()
    }

    private fun serve(socket: Socket) {
        socket.soTimeout = 10_000
        val input = socket.getInputStream()
        val reader = BufferedReader(InputStreamReader(input, Charsets.ISO_8859_1))
        val requestLine = reader.readLine() ?: run { socket.close(); return }
        val parts = requestLine.split(' ')
        if (parts.size < 2) { socket.close(); return }
        val headers = HashMap<String, String>()
        while (true) {
            val line = reader.readLine() ?: break
            if (line.isEmpty()) break
            val i = line.indexOf(':')
            if (i > 0) headers[line.substring(0, i).trim().lowercase()] = line.substring(i + 1).trim()
        }
        // Drain a request body (POST /auth/login) so the client sees a clean response.
        headers["content-length"]?.toIntOrNull()?.let { n -> var left = n; while (left > 0 && reader.read() >= 0) left-- }
        handle(HttpExchange(parts[0], URI(parts[1]), headers, socket))
    }

    private fun query(ex: HttpExchange): Map<String, String> =
        (ex.requestURI.rawQuery ?: "").split('&').filter { it.contains('=') }.associate { kv ->
            val (k, v) = kv.split('=', limit = 2)
            URLDecoder.decode(k, "UTF-8") to URLDecoder.decode(v, "UTF-8")
        }

    private fun handle(ex: HttpExchange) {
        val path = ex.requestURI.path
        val q = query(ex)
        val endpoint = path.substringAfterLast('/').removeSuffix(".view")
        calls += endpoint
        try {
            if (path.startsWith("/auth/login") || path.startsWith("/api/")) { respond(ex, 404, "{}".toByteArray(), "application/json"); return }
            if (!path.contains("/rest/")) { respond(ex, 404, ByteArray(0)); return }
            // Token auth: t = md5(password + salt).
            val salt = q["s"] ?: ""
            val expectedToken = md5(expectedPassword + salt)
            if (q["u"] != expectedUser || q["t"] != expectedToken) { respond(ex, 200, authError.toByteArray(), "application/json"); return }
            val (status, body, type) = when (endpoint) {
                "ping" -> Triple(200, fixture("ping"), "application/json")
                "getOpenSubsonicExtensions" -> Triple(200, fixture("extensions"), "application/json")
                "getMusicFolders" -> Triple(200, wrap("""\"musicFolders\":{\"musicFolder\":[{\"id\":1,\"name\":\"Music\"}]}"""), "application/json")
                "getArtists" -> Triple(200, fixture("artists"), "application/json")
                "getArtist" -> Triple(200, wrap("""\"artist\":{\"id\":\"ar1\",\"name\":\"Boards of Canada\",\"albumCount\":2,\"album\":${albumsJson()}}"""), "application/json")
                "getAlbumList2" -> Triple(200, if ((q["offset"]?.toIntOrNull() ?: 0) == 0) fixture("album_list2") else wrap("\"albumList2\":{\"album\":[]}"), "application/json")
                "getAlbum" -> Triple(200, wrap("""\"album\":${albumWithSongs(q["id"] ?: "al1")}"""), "application/json")
                "getSong" -> Triple(200, wrap("""\"song\":${songJson(q["id"] ?: "s1")}"""), "application/json")
                "search3" -> Triple(200, if ((q["songOffset"]?.toIntOrNull() ?: 0) == 0 && (q["albumOffset"]?.toIntOrNull() ?: 0) == 0 && (q["artistOffset"]?.toIntOrNull() ?: 0) == 0) fixture("search3") else fixture("search3_empty"), "application/json")
                "getPlaylists" -> Triple(200, fixture("playlists"), "application/json")
                "getPlaylist" -> Triple(200, fixture("playlist"), "application/json")
                "getGenres" -> Triple(200, fixture("genres"), "application/json")
                "getScanStatus" -> Triple(200, fixture("scan_status"), "application/json")
                "getStarred2" -> Triple(200, wrap("\"starred2\":{\"artist\":[],\"album\":[],\"song\":[]}"), "application/json")
                "getLyricsBySongId" -> Triple(200, fixture("lyrics_v2"), "application/json")
                "getSimilarSongs2" -> Triple(200, wrap("\"similarSongs2\":{\"song\":[]}"), "application/json")
                "getTopSongs" -> Triple(200, wrap("\"topSongs\":{\"song\":[]}"), "application/json")
                "getRandomSongs" -> Triple(200, wrap("\"randomSongs\":{\"song\":[]}"), "application/json")
                "getCoverArt" -> Triple(200, png(), "image/png")
                "stream", "download" -> Triple(200, wav(), "audio/wav")
                "setRating" -> { q["id"]?.let { ratings[it] = q["rating"]?.toIntOrNull() ?: 0 }; Triple(200, ok.toByteArray(), "application/json") }
                "star" -> { q["id"]?.let { starred += it }; Triple(200, ok.toByteArray(), "application/json") }
                "unstar" -> { q["id"]?.let { starred -= it }; Triple(200, ok.toByteArray(), "application/json") }
                "scrobble" -> { q["id"]?.let { scrobbles += it to (q["submission"] != "false") }; Triple(200, ok.toByteArray(), "application/json") }
                "getNowPlaying" -> Triple(200, wrap("\"nowPlaying\":{\"entry\":[]}"), "application/json")
                "savePlayQueue", "createPlaylist", "updatePlaylist", "deletePlaylist" -> Triple(200, ok.toByteArray(), "application/json")
                else -> Triple(200, wrap("\"error\":{\"code\":70,\"message\":\"unknown endpoint $endpoint\"}").let { it }, "application/json")
            }
            respond(ex, status, body, type)
        } catch (e: Exception) {
            respond(ex, 500, (e.message ?: "error").toByteArray(), "text/plain")
        }
    }

    private fun respond(ex: HttpExchange, status: Int, body: ByteArray, type: String = "application/json") = ex.respond(status, body, type)

    private fun wrap(inner: String) = """{"subsonic-response":{"status":"ok","version":"1.16.1","type":"navidrome","serverVersion":"0.63.1 (abcd1234)","openSubsonic":true,$inner}}""".toByteArray()

    private fun songJson(id: String): String {
        val n = id.removePrefix("s").toIntOrNull() ?: 1
        return """{"id":"$id","parent":"al1","isDir":false,"title":"Song $n","album":"Music Has the Right","artist":"Boards of Canada","albumId":"al1","artistId":"ar1","track":$n,"year":1998,"genre":"Electronic","coverArt":"al1","duration":4,"bitRate":320,"suffix":"wav","contentType":"audio/wav","size":80000,"path":"boc/$id.wav","created":"2024-01-01T00:00:00Z","type":"music","mediaType":"song"}"""
    }

    private fun albumsJson() = """[{"id":"al1","name":"Music Has the Right","artist":"Boards of Canada","artistId":"ar1","coverArt":"al1","songCount":2,"duration":8,"created":"2024-01-01T00:00:00Z","year":1998,"genre":"Electronic"},{"id":"al2","name":"Geogaddi","artist":"Boards of Canada","artistId":"ar1","coverArt":"al2","songCount":0,"duration":0,"created":"2024-01-01T00:00:00Z","year":2002,"genre":"Electronic"}]"""

    private fun albumWithSongs(id: String) = if (id == "al1")
        """{"id":"al1","name":"Music Has the Right","artist":"Boards of Canada","artistId":"ar1","coverArt":"al1","songCount":2,"duration":8,"created":"2024-01-01T00:00:00Z","year":1998,"genre":"Electronic","song":[${songJson("s1")},${songJson("s2")}]}"""
    else """{"id":"$id","name":"Geogaddi","artist":"Boards of Canada","artistId":"ar1","coverArt":"$id","songCount":0,"duration":0,"created":"2024-01-01T00:00:00Z","year":2002,"song":[]}"""

    private fun md5(s: String): String = java.security.MessageDigest.getInstance("MD5").digest(s.toByteArray()).joinToString("") { "%02x".format(it) }

    /** A 1x1 opaque PNG. */
    private fun png(): ByteArray = byteArrayOf(
        0x89.toByte(), 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0x0D, 0x49, 0x48, 0x44, 0x52, 0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0, 0x90.toByte(), 0x77, 0x53, 0xDE.toByte(),
        0, 0, 0, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7.toByte(), 0x63, 0xF8.toByte(), 0xCF.toByte(), 0xC0.toByte(), 0, 0, 0x03, 0x01, 0x01, 0, 0x18, 0xDD.toByte(), 0x8D.toByte(), 0xB0.toByte(),
        0, 0, 0, 0, 0x49, 0x45, 0x4E, 0x44, 0xAE.toByte(), 0x42, 0x60, 0x82.toByte(),
    )

    /** 0.2 s of silence, 8 kHz mono 16-bit. */
    private fun wav(): ByteArray {
        val samples = 1600
        val data = ByteArray(samples * 2)
        val out = ByteArrayOutputStream()
        fun le32(v: Int) = byteArrayOf((v and 0xff).toByte(), ((v shr 8) and 0xff).toByte(), ((v shr 16) and 0xff).toByte(), ((v shr 24) and 0xff).toByte())
        fun le16(v: Int) = byteArrayOf((v and 0xff).toByte(), ((v shr 8) and 0xff).toByte())
        out.write("RIFF".toByteArray()); out.write(le32(36 + data.size)); out.write("WAVE".toByteArray())
        out.write("fmt ".toByteArray()); out.write(le32(16)); out.write(le16(1)); out.write(le16(1)); out.write(le32(8000)); out.write(le32(16000)); out.write(le16(2)); out.write(le16(16))
        out.write("data".toByteArray()); out.write(le32(data.size)); out.write(data)
        return out.toByteArray()
    }
}
