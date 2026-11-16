package app.hocket.ui.screens.setup

import java.net.URLDecoder

/**
 * The links one device hands another (the desktop reads the same forms,
 * desktop/src/shared/deep-link.ts):
 *
 * - a pairing invite from Add a device: `hocket://pair?d=<base64url JSON>`, which the core reads
 *   (the identity, the inviter's key, a one-time secret, where to reach it, account configs
 *   without passwords). Kept whole; the core checks it.
 * - a coordinator claim, as `hocket-coordinator pair --url` prints it:
 *   `hocket://claim?url=<coordinator>&code=<code>`.
 */
object PairingLinks {
    private val PAIR = Regex("""hocket://pair\?d=[A-Za-z0-9_-]{16,6000}""", RegexOption.IGNORE_CASE)

    /** The pairing link in [text] (scanned, pasted, or opened), or null. */
    fun pair(text: String?): String? = text?.let { PAIR.find(it)?.value }

    /** `(url, code)` of a claim link, or null; `url` may be empty. */
    fun claim(text: String?): Pair<String, String>? {
        val t = text?.trim() ?: return null
        if (!t.startsWith("hocket://claim?", ignoreCase = true)) return null
        val values = t.substringAfter('?').split('&').mapNotNull { p ->
            val i = p.indexOf('=')
            if (i <= 0) null else p.substring(0, i) to runCatching { URLDecoder.decode(p.substring(i + 1).replace("+", "%2B"), "UTF-8") }.getOrNull()
        }.toMap()
        val code = values["code"]?.trim()?.takeIf { Regex("""[A-Za-z0-9 -]{4,32}""").matches(it) } ?: return null
        val url = values["url"]?.trim().orEmpty()
        if (url.isNotEmpty() && !Regex("""wss?://\S+""", RegexOption.IGNORE_CASE).matches(url)) return null
        return url to code
    }
}
