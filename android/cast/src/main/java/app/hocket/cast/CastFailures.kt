package app.hocket.cast

import android.content.Context
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import java.net.URI

/**
 * What a receiver failure means, per session (docs/providers.md, Phase 5).
 *
 * - Navidrome: a failure before anything has played means the server is out of the receiver's
 *   reach, and ends the session ([Action.Unreachable]); later ones go to the core, which retries
 *   once and then skips.
 * - YouTube Music: the receiver fetches googlevideo URLs this phone resolved, and they only work
 *   from the public address that resolved them. A failure goes to the core, which fetches a fresh
 *   URL and retries once, then skips. Two failures before any YouTube Music item has played on this
 *   receiver are taken as that address mismatch: before anything else played the session ends
 *   with the explanation ([Action.YtmUnreachable]); after, the explanation is shown once and
 *   Navidrome items carry on ([Action.ReportWithYtmHint]).
 */
internal class CastFailures {
    enum class Action { Report, Unreachable, YtmUnreachable, ReportWithYtmHint }

    private var played = false
    private var ytmPlayed = false
    private var ytmMisses = 0

    fun reset() {
        played = false
        ytmPlayed = false
        ytmMisses = 0
    }

    fun playing(ytm: Boolean) {
        played = true
        if (ytm) {
            ytmPlayed = true
            ytmMisses = 0
        }
    }

    fun failed(ytm: Boolean): Action {
        if (!ytm) return if (played) Action.Report else Action.Unreachable
        if (ytmPlayed) return Action.Report
        ytmMisses++
        return when {
            ytmMisses < 2 -> Action.Report
            !played -> Action.YtmUnreachable
            ytmMisses == 2 -> Action.ReportWithYtmHint
            else -> Action.Report
        }
    }

    /** Why a YouTube Music URL likely didn't work on the receiver, from this phone's network. */
    enum class Cause { MobileData, Vpn, Other }

    companion object {
        /** A direct YouTube Music (googlevideo) stream. */
        fun isYtm(url: String): Boolean =
            runCatching { URI(url).host }.getOrNull()?.endsWith(".googlevideo.com") == true

        fun cause(cellular: Boolean, vpn: Boolean): Cause = when {
            vpn -> Cause.Vpn
            cellular -> Cause.MobileData
            else -> Cause.Other
        }

        fun hint(context: Context, name: String): String {
            val cm = context.getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager
            val caps = runCatching { cm?.getNetworkCapabilities(cm.activeNetwork) }.getOrNull()
            val c = cause(
                cellular = caps?.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR) == true,
                vpn = caps?.hasTransport(NetworkCapabilities.TRANSPORT_VPN) == true,
            )
            val res = when (c) {
                Cause.MobileData -> R.string.cast_ytm_mobile_data
                Cause.Vpn -> R.string.cast_ytm_vpn
                Cause.Other -> R.string.cast_ytm_address
            }
            return context.getString(res, name)
        }
    }
}
