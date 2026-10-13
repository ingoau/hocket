package app.hocket.core

import app.hocket.core.api.Command
import app.hocket.core.api.Filter
import app.hocket.core.api.FilterField
import app.hocket.core.api.FilterNode
import app.hocket.core.api.FilterOp
import app.hocket.core.api.FilterRule
import app.hocket.core.api.FilterValue
import app.hocket.core.api.MediaSessionAction
import app.hocket.core.api.ServerId
import app.hocket.core.api.SortOrder

/**
 * Library-wide plays that need no browsing: launcher shortcuts and the media browser's suggestions
 * start them. Each is an ordinary command; the shuffles are filter contexts evaluated by the core
 * against its local mirror (so they work offline), with fixed filter ids so a restored saved queue
 * re-resolves to the same selection.
 */
object QuickPlay {
    const val LOVED_FILTER_ID = "quickplay.loved"
    const val ALL_FILTER_ID = "quickplay.all"
    const val OFFLINE_FILTER_ID = "quickplay.offline"

    /** Carry on with the session (the core decides what that is; nothing loaded does nothing). */
    fun resume(): Command = Commands.mediaSessionCommand(MediaSessionAction.Play)

    fun shuffleLoved(serverId: ServerId, label: String): Command = shuffle(serverId, filter(LOVED_FILTER_ID, label, rule(FilterField.Loved)))

    /** Every song: an empty `All` matches everything. */
    fun shuffleAll(serverId: ServerId, label: String): Command = shuffle(serverId, filter(ALL_FILTER_ID, label, FilterNode.All(emptyList())))

    /** Songs playable with no network (downloaded or fully cached). */
    fun shuffleOffline(serverId: ServerId, label: String): Command = shuffle(serverId, filter(OFFLINE_FILTER_ID, label, rule(FilterField.AvailableOffline)))

    private fun shuffle(serverId: ServerId, filter: Filter): Command = Commands.playContext(Commands.filterContext(serverId, filter), shuffle = true)

    private fun filter(id: String, label: String, root: FilterNode) = Filter(id = id, name = label, root = root, sort = SortOrder.Random, descending = false, limit = null)

    private fun rule(field: FilterField) = FilterNode.Rule(FilterRule(field, FilterOp.IsTrue, FilterValue.Bool(true)))
}
