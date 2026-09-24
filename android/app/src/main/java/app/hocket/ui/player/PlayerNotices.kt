package app.hocket.ui.player

import androidx.annotation.StringRes
import androidx.compose.runtime.Composable
import androidx.compose.ui.res.stringResource
import app.hocket.R
import app.hocket.core.api.EventPlayerNoticeInner
import app.hocket.core.api.PlayerNoticeCode

/**
 * Player notices are matched on the core's stable [PlayerNoticeCode] and shown from this app's
 * strings; the core's English message is only the fallback for a code this build doesn't know
 * (or a notice without one).
 */
@StringRes
internal fun PlayerNoticeCode.stringRes(): Int = when (this) {
    PlayerNoticeCode.OfflineSkipping -> R.string.player_notice_offline_skipping
    PlayerNoticeCode.NothingAvailableOffline -> R.string.player_notice_offline_nothing
    PlayerNoticeCode.NoServer -> R.string.player_notice_no_server
    PlayerNoticeCode.PlaybackProblem -> R.string.player_notice_playback_problem
    PlayerNoticeCode.CouldNotPlaySkipped -> R.string.player_notice_could_not_play_skipped
    PlayerNoticeCode.CouldNotPlayStopped -> R.string.player_notice_could_not_play_stopped
    PlayerNoticeCode.AutoplayFoundNothing -> R.string.player_notice_autoplay_found_nothing
}

/** Codes whose text takes the notice's detail (a song title or the player's message). */
private val WITH_DETAIL = setOf(PlayerNoticeCode.PlaybackProblem, PlayerNoticeCode.CouldNotPlaySkipped, PlayerNoticeCode.CouldNotPlayStopped)

/** The offline notices, which also show as a snackbar (AppRoot). */
val OFFLINE_NOTICE_CODES = setOf(PlayerNoticeCode.OfflineSkipping, PlayerNoticeCode.NothingAvailableOffline)

/** The localised text of a notice; null when there is nothing to show. */
@Composable
fun playerNoticeText(notice: EventPlayerNoticeInner?): String? {
    notice ?: return null
    val code = notice.code
    return when {
        code == null -> notice.message
        code in WITH_DETAIL && notice.detail == null -> notice.message
        code in WITH_DETAIL -> stringResource(code.stringRes(), notice.detail!!)
        else -> stringResource(code.stringRes())
    }
}
