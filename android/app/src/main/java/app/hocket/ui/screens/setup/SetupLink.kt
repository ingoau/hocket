package app.hocket.ui.screens.setup

import java.io.ByteArrayOutputStream
import java.net.URLDecoder
import java.net.URLEncoder
import java.util.Base64
import java.util.zip.Deflater
import java.util.zip.Inflater

/**
 * What one device hands another so it can be set up without typing (Set up another device): the
 * Navidrome server and who signs in there, the password when the person chose to include it, and
 * the coordinator when this device has one, and otherwise the account settings changed from their
 * defaults (`Query.SetupSettings`: a JSON object, which a coordinator would have synced). As a link
 * (the setup code's content):
 * `hocket://setup?server=<url>&user=<name>[&password=<p>][&coordinator=<url>][&settings=<s>]`, each
 * value percent-encoded; `settings` is the JSON zlib-deflated and in unpadded base64url, to keep the
 * code small. The desktop app reads and writes the same form (desktop/src/shared/deep-link.ts).
 */
data class SetupLink(
    val server: String,
    val username: String,
    val password: String? = null,
    val coordinator: String? = null,
    val settings: String? = null,
) {
    fun format(): String = buildString {
        append(PREFIX)
        append("server=").append(enc(server))
        append("&user=").append(enc(username))
        password?.let { append("&password=").append(enc(it)) }
        coordinator?.let { append("&coordinator=").append(enc(it)) }
        settings?.let { append("&settings=").append(packSettings(it)) }
    }

    /** Never the password, in case one ends up in a log. */
    override fun toString(): String =
        "SetupLink(server=$server, username=$username, password=${if (password == null) "none" else "***"}, coordinator=$coordinator, settings=${settings?.length ?: "none"})"

    companion object {
        private const val PREFIX = "hocket://setup?"

        /**
         * The setup link in [text] (surrounding whitespace allowed), or null when it is not one: a
         * different link, no `http(s)://` server, or no user. Unknown parameters are ignored.
         */
        fun parse(text: String?): SetupLink? {
            val trimmed = text?.trim() ?: return null
            if (!trimmed.startsWith(PREFIX, ignoreCase = true)) return null
            val params = trimmed.substring(PREFIX.length).split('&').mapNotNull { pair ->
                val i = pair.indexOf('=')
                if (i <= 0) null else runCatching { pair.substring(0, i) to URLDecoder.decode(pair.substring(i + 1), "UTF-8") }.getOrNull()
            }.toMap()
            // Settings that don't unpack are dropped, not the whole link: the device can still sign in.
            val settings = params["settings"]?.let(::unpackSettings)
            val server = params["server"]?.let(::normaliseServerUrl) ?: return null
            val username = params["user"]?.trim()?.takeIf { it.isNotEmpty() } ?: return null
            return SetupLink(
                server = server,
                username = username,
                password = params["password"]?.takeIf { it.isNotEmpty() },
                coordinator = params["coordinator"]?.trim()?.takeIf { it.isNotEmpty() },
                settings = settings,
            )
        }

        /** Settings JSON → zlib → unpadded base64url (all link-safe, so no percent-encoding). */
        internal fun packSettings(json: String): String {
            val deflater = Deflater(Deflater.BEST_COMPRESSION)
            try {
                deflater.setInput(json.toByteArray(Charsets.UTF_8))
                deflater.finish()
                val out = ByteArrayOutputStream()
                val buf = ByteArray(1024)
                while (!deflater.finished()) out.write(buf, 0, deflater.deflate(buf))
                return Base64.getUrlEncoder().withoutPadding().encodeToString(out.toByteArray())
            } finally {
                deflater.end()
            }
        }

        /** [packSettings] undone, or null when [packed] is not a packed JSON object (or unpacks past 64 KiB). */
        internal fun unpackSettings(packed: String): String? = runCatching {
            val inflater = Inflater()
            try {
                inflater.setInput(Base64.getUrlDecoder().decode(packed.trim().trimEnd('=')))
                val out = ByteArrayOutputStream()
                val buf = ByteArray(1024)
                while (!inflater.finished()) {
                    val n = inflater.inflate(buf)
                    if (n == 0 && (inflater.needsInput() || inflater.needsDictionary())) return@runCatching null
                    out.write(buf, 0, n)
                    if (out.size() > MAX_SETTINGS_BYTES) return@runCatching null
                }
                out.toString(Charsets.UTF_8.name()).takeIf { it.trimStart().startsWith("{") }
            } finally {
                inflater.end()
            }
        }.getOrNull()

        private const val MAX_SETTINGS_BYTES = 64 * 1024

        // URLEncoder writes form encoding (a space as "+"); %20 reads the same everywhere.
        private fun enc(s: String) = URLEncoder.encode(s, "UTF-8").replace("+", "%20")
    }
}
