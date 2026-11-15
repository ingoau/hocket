package app.hocket.ui.screens.settings

import app.hocket.core.api.Command
import app.hocket.core.api.CommandSignCoordinatorInToYoutubeMusicInner
import app.hocket.core.api.EventCoordinatorYtmStatusInner

/**
 * The coordinator's YouTube Music analysis row in Settings → Connect's devices (docs/providers.md,
 * Phase 4 follow-up), as the desktop's coordinator settings show it: only for the owner identity
 * and only when the coordinator runs it (`--analysis-ytm`).
 */
data class CoordinatorYtmRow(val signedIn: Boolean, val account: String?, val error: String?)

object CoordinatorYtm {
    /** The row to show, or null: not the owner, not asked yet, or the analysis is off. */
    fun row(owner: Boolean, status: EventCoordinatorYtmStatusInner?): CoordinatorYtmRow? =
        if (!owner || status == null || !status.enabled) null
        else CoordinatorYtmRow(status.signed_in, status.account?.takeIf { it.isNotBlank() }, status.error?.takeIf { it.isNotBlank() })

    /**
     * What a finished sign-in sends: the fresh session's `Cookie:` header for the core to seal to
     * the coordinator's key, or null when the sign-in was cancelled or brought no session.
     */
    fun signIn(ok: Boolean, cookies: String?): Command? =
        if (ok && !cookies.isNullOrBlank()) Command.SignCoordinatorInToYoutubeMusic(CommandSignCoordinatorInToYoutubeMusicInner(cookies)) else null

    /** Back to anonymous. */
    val signOut: Command = Command.SignCoordinatorInToYoutubeMusic(CommandSignCoordinatorInToYoutubeMusicInner(null))
}
