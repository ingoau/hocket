package app.hocket.playback

import java.security.MessageDigest

/**
 * The core's `AccountId` for a login, derived the same way on every device (the core's
 * `server_id_for`): the first 8 bytes of md5(`serverKey(url)|username`), in hex.
 */
object AccountIds {
    /** A server URL as the core's `server_key` normalises it: blanks and trailing slashes go, scheme and host lowercased. */
    fun serverKey(url: String): String {
        val u = url.trim().trimEnd('/')
        val start = u.indexOf("://").let { if (it >= 0) it + 3 else 0 }
        val rest = u.substring(start).indexOfFirst { it == '/' || it == '?' || it == '#' }
        val end = if (rest < 0) u.length else start + rest
        return u.substring(0, end).lowercase() + u.substring(end)
    }

    fun navidrome(url: String, username: String): String {
        val digest = MessageDigest.getInstance("MD5").digest("${serverKey(url)}|$username".toByteArray(Charsets.UTF_8))
        return digest.take(8).joinToString("") { "%02x".format(it) }
    }
}
