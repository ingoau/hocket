package app.hocket.playback

/**
 * This device's YouTube Music sign-in as the credential store keeps it: one entry under a fixed
 * (url, username) key, whose "password" is the sign-in's cookie header (empty: anonymous). The
 * core keys the account by its channel id, known only once it checked the cookies, so the store
 * can't use the account id. Never synced to other devices: each signs in itself.
 */
object YtmLogin {
    const val URL = "https://music.youtube.com"
    const val USER = "ytm"
    val KEY = URL to USER

    fun isYtm(c: ServerCredential) = c.url == URL && c.username == USER

    fun credential(cookies: String?) = ServerCredential(URL, USER, cookies.orEmpty(), null)

    /** Whether a `Cookie:` header holds a signed-in YouTube session (`SAPISID` or its secure twin). */
    fun hasSession(cookieHeader: String?): Boolean =
        cookieHeader.orEmpty().split(';').map { it.trim() }.any { part ->
            val (name, value) = part.split('=', limit = 2).let { it[0] to it.getOrElse(1) { "" } }
            (name == "SAPISID" || name == "__Secure-3PAPISID") && value.isNotEmpty()
        }
}
