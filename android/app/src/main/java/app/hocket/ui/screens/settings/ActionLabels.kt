package app.hocket.ui.screens.settings

import androidx.annotation.StringRes
import androidx.compose.runtime.Composable
import androidx.compose.ui.res.stringResource
import app.hocket.R
import app.hocket.core.ActionIds

/**
 * Human labels for the registry's customisable action ids (the wording of hocket-core
 * `actions/defs.rs`, as Android strings so they translate). Ids without a string fall back to the
 * registry's own descriptor label when the caller has one, else to the id split into words.
 */
object ActionLabels {
    @StringRes
    fun res(id: String): Int? = when (id) {
        ActionIds.PLAY -> R.string.action_label_play
        ActionIds.PLAY_SHUFFLED -> R.string.action_label_play_shuffled
        ActionIds.PLAY_NEXT -> R.string.action_label_play_next
        ActionIds.PLAY_LATER -> R.string.action_label_play_later
        ActionIds.ADD_TO_PLAYLIST -> R.string.action_label_add_to_playlist
        ActionIds.LOVE -> R.string.action_label_love
        ActionIds.UNLOVE -> R.string.action_label_unlove
        ActionIds.rate(0) -> R.string.action_label_rate0
        ActionIds.rate(1) -> R.string.action_label_rate1
        ActionIds.rate(2) -> R.string.action_label_rate2
        ActionIds.rate(3) -> R.string.action_label_rate3
        ActionIds.rate(4) -> R.string.action_label_rate4
        ActionIds.rate(5) -> R.string.action_label_rate5
        ActionIds.DOWNLOAD -> R.string.action_label_download
        ActionIds.UNPIN -> R.string.action_label_unpin
        ActionIds.GO_TO_ALBUM -> R.string.action_label_go_to_album
        ActionIds.GO_TO_ARTIST -> R.string.action_label_go_to_artist
        ActionIds.REMOVE_FROM_QUEUE -> R.string.action_label_remove_from_queue
        ActionIds.REMOVE_FROM_PLAYLIST -> R.string.action_label_remove_from_playlist
        ActionIds.RESTORE_SAVED_QUEUE -> R.string.action_label_restore_saved_queue
        ActionIds.PIN_SAVED_QUEUE -> R.string.action_label_pin_saved_queue
        ActionIds.UNPIN_SAVED_QUEUE -> R.string.action_label_unpin_saved_queue
        ActionIds.SAVE_QUEUE_AS_PLAYLIST -> R.string.action_label_save_queue_as_playlist
        ActionIds.DELETE_SAVED_QUEUE -> R.string.action_label_delete_saved_queue
        ActionIds.DELETE_PLAYLIST -> R.string.action_label_delete_playlist
        ActionIds.PREVIOUS -> R.string.action_label_previous
        ActionIds.TOGGLE_PLAY -> R.string.action_label_toggle_play
        ActionIds.NEXT -> R.string.action_label_next
        ActionIds.SHUFFLE -> R.string.action_label_shuffle
        ActionIds.REPEAT -> R.string.action_label_repeat
        else -> null
    }

    /** "playShuffled" -> "Play shuffled": the last resort, never a raw camel-case id on screen. */
    fun humanise(id: String): String =
        id.replace(Regex("(?<=[a-z])(?=[A-Z0-9])"), " ").lowercase().replaceFirstChar { it.uppercase() }
}

@Composable
fun actionLabel(id: String, registryLabel: String? = null): String =
    ActionLabels.res(id)?.let { stringResource(it) } ?: registryLabel ?: ActionLabels.humanise(id)
