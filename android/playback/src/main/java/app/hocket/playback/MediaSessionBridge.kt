package app.hocket.playback

import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.os.Bundle
import androidx.media3.common.MediaItem
import androidx.media3.session.CommandButton
import androidx.media3.session.LibraryResult
import androidx.media3.session.MediaLibraryService
import androidx.media3.session.MediaSession
import androidx.media3.session.SessionCommand
import androidx.media3.session.SessionError
import androidx.media3.session.SessionResult
import app.hocket.core.Commands
import app.hocket.core.api.Command
import app.hocket.core.api.MediaSessionAction
import app.hocket.core.api.MediaSessionState
import app.hocket.core.api.QueueView
import app.hocket.core.api.RepeatMode
import com.google.common.collect.ImmutableList
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import com.google.common.util.concurrent.SettableFuture
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch

/**
 * Builds the Media3 [MediaLibrarySession] over a [CoreSessionPlayer] and keeps its custom button
 * layout in step with the core's customised action list (`MediaSessionState.actions`): love,
 * shuffle, repeat and rate as media button preferences, so they appear on the notification and on
 * Wear/Auto surfaces that honour them. Transport buttons come from the player's available commands.
 *
 * The library side ([browser]) answers other apps' browsing and search (Android Auto, Wear,
 * Assistant, any `MediaBrowser`); a "play …" search request (voice, Auto's search) is resolved to one
 * library item here, before the player dispatches it. Every connected controller may open artwork
 * URIs ([ArtworkProvider.allow]). The "recent" root (system playback resumption) is refused, as
 * resumption is.
 */
class MediaSessionBridge(
    context: Context,
    val player: CoreSessionPlayer,
    private val dispatch: (Command) -> Unit,
    launchIntent: Intent?,
    private val browser: LibraryBrowser,
    private val scope: CoroutineScope,
) {
    companion object {
        const val CMD_LOVE = "app.hocket.LOVE"
        const val CMD_SHUFFLE = "app.hocket.SHUFFLE"
        const val CMD_REPEAT = "app.hocket.REPEAT"
        const val CMD_RATE = "app.hocket.RATE"
        const val EXTRA_RATING = "rating"
    }

    private val customCommands = listOf(CMD_LOVE, CMD_SHUFFLE, CMD_REPEAT, CMD_RATE).map { SessionCommand(it, Bundle.EMPTY) }

    /** Runs a browse request on [scope] and completes the future with its result. */
    private fun <T> async(block: suspend () -> T): ListenableFuture<T> {
        val future = SettableFuture.create<T>()
        scope.launch { runCatching { block() }.fold({ future.set(it) }, { future.setException(it) }) }
        return future
    }

    val session: MediaLibraryService.MediaLibrarySession = MediaLibraryService.MediaLibrarySession.Builder(context, player, object : MediaLibraryService.MediaLibrarySession.Callback {
            override fun onConnect(session: MediaSession, controller: MediaSession.ControllerInfo): MediaSession.ConnectionResult {
                ArtworkProvider.allow(controller.packageName)
                val commands = MediaSession.ConnectionResult.DEFAULT_SESSION_COMMANDS.buildUpon().apply { customCommands.forEach { add(it) } }.build()
                return MediaSession.ConnectionResult.AcceptedResultBuilder(session)
                    .setAvailableSessionCommands(commands)
                    .setMediaButtonPreferences(buttons(player.state))
                    .build()
            }

            override fun onCustomCommand(session: MediaSession, controller: MediaSession.ControllerInfo, customCommand: SessionCommand, args: Bundle): ListenableFuture<SessionResult> {
                when (customCommand.customAction) {
                    CMD_LOVE -> dispatch(Commands.mediaSessionCommand(MediaSessionAction.Love))
                    CMD_SHUFFLE -> dispatch(Commands.mediaSessionCommand(MediaSessionAction.Shuffle))
                    CMD_REPEAT -> dispatch(Commands.mediaSessionCommand(MediaSessionAction.Repeat))
                    CMD_RATE -> dispatch(Commands.mediaSessionCommand(MediaSessionAction.Rate, args.getInt(EXTRA_RATING, 0).toDouble()))
                    else -> return Futures.immediateFuture(SessionResult(SessionResult.RESULT_ERROR_NOT_SUPPORTED))
                }
                return Futures.immediateFuture(SessionResult(SessionResult.RESULT_SUCCESS))
            }

            override fun onPlaybackResumption(session: MediaSession, controller: MediaSession.ControllerInfo): ListenableFuture<MediaSession.MediaItemsWithStartPosition> {
                // Never auto-resume from the system (design: resuming is always explicit). Report what we
                // have so the notification can stay; the core decides whether anything plays.
                return Futures.immediateFuture(MediaSession.MediaItemsWithStartPosition(emptyList(), 0, 0L))
            }

            override fun onGetLibraryRoot(session: MediaLibraryService.MediaLibrarySession, browser: MediaSession.ControllerInfo, params: MediaLibraryService.LibraryParams?): ListenableFuture<LibraryResult<MediaItem>> {
                if (params?.isRecent == true) return Futures.immediateFuture(LibraryResult.ofError(SessionError.ERROR_NOT_SUPPORTED))
                return Futures.immediateFuture(LibraryResult.ofItem(this@MediaSessionBridge.browser.root(), MediaLibraryService.LibraryParams.Builder().setExtras(LibraryBrowser.rootExtras()).build()))
            }

            override fun onGetChildren(session: MediaLibraryService.MediaLibrarySession, browser: MediaSession.ControllerInfo, parentId: String, page: Int, pageSize: Int, params: MediaLibraryService.LibraryParams?): ListenableFuture<LibraryResult<ImmutableList<MediaItem>>> = async {
                val children = this@MediaSessionBridge.browser.children(parentId, page, pageSize)
                if (children == null) LibraryResult.ofError(SessionError.ERROR_BAD_VALUE) else LibraryResult.ofItemList(children, params)
            }

            override fun onGetItem(session: MediaLibraryService.MediaLibrarySession, browser: MediaSession.ControllerInfo, mediaId: String): ListenableFuture<LibraryResult<MediaItem>> = async {
                this@MediaSessionBridge.browser.item(mediaId)?.let { LibraryResult.ofItem(it, null) } ?: LibraryResult.ofError(SessionError.ERROR_BAD_VALUE)
            }

            override fun onSearch(session: MediaLibraryService.MediaLibrarySession, browser: MediaSession.ControllerInfo, query: String, params: MediaLibraryService.LibraryParams?): ListenableFuture<LibraryResult<Void>> {
                scope.launch {
                    val count = runCatching { this@MediaSessionBridge.browser.search(query).size }.getOrDefault(0)
                    session.notifySearchResultChanged(browser, query, count, params)
                }
                return Futures.immediateFuture(LibraryResult.ofVoid())
            }

            override fun onGetSearchResult(session: MediaLibraryService.MediaLibrarySession, browser: MediaSession.ControllerInfo, query: String, page: Int, pageSize: Int, params: MediaLibraryService.LibraryParams?): ListenableFuture<LibraryResult<ImmutableList<MediaItem>>> = async {
                val all = this@MediaSessionBridge.browser.search(query)
                val from = (page.toLong() * pageSize).coerceIn(0, all.size.toLong()).toInt()
                LibraryResult.ofItemList(all.subList(from, (from + pageSize.coerceAtLeast(0)).coerceAtMost(all.size)), params)
            }

            override fun onAddMediaItems(mediaSession: MediaSession, controller: MediaSession.ControllerInfo, mediaItems: MutableList<MediaItem>): ListenableFuture<MutableList<MediaItem>> = async {
                // Library ids pass through to the player; a search request ("play X") becomes the item
                // it names. Anything unresolvable is dropped (the player then does nothing).
                mediaItems.mapNotNull { item ->
                    val search = item.requestMetadata.searchQuery
                    when {
                        search != null -> this@MediaSessionBridge.browser.resolveSearch(search, item.requestMetadata.extras)
                        item.mediaId.isNotEmpty() -> item
                        else -> null
                    }
                }.toMutableList()
            }
        })
        .setId("hocket")
        .apply {
            launchIntent?.let {
                setSessionActivity(PendingIntent.getActivity(context, 0, it, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT))
            }
        }
        .build()

    /** Push a new queue view to the player (its playlist). Main thread. */
    fun applyQueue(queue: QueueView) = player.applyQueue(queue)

    /**
     * The library changed: browsers subscribed to a section refetch it. The count is Media3's
     * "unknown" (`Int.MAX_VALUE`): knowing it would take a query per section.
     */
    fun libraryChanged() {
        for (section in LibrarySection.entries) session.notifyChildrenChanged(section.id, Int.MAX_VALUE, null)
    }

    /** Push a new state to the player and refresh the custom layout. Main thread. */
    fun apply(state: MediaSessionState, clockOffsetMs: Double, remote: Boolean = player.isRemote) {
        player.apply(state, clockOffsetMs, remote)
        session.setMediaButtonPreferences(buttons(state))
    }

    private fun buttons(state: MediaSessionState): List<CommandButton> {
        val meta = state.metadata
        return state.actions.mapNotNull { action ->
            when (action) {
                MediaSessionAction.Love -> CommandButton.Builder(if (meta?.loved == true) CommandButton.ICON_HEART_FILLED else CommandButton.ICON_HEART_UNFILLED)
                    .setDisplayName(if (meta?.loved == true) "Unlove" else "Love")
                    .setSessionCommand(SessionCommand(CMD_LOVE, Bundle.EMPTY)).setEnabled(meta != null).build()
                MediaSessionAction.Shuffle -> CommandButton.Builder(if (state.shuffle) CommandButton.ICON_SHUFFLE_ON else CommandButton.ICON_SHUFFLE_OFF)
                    .setDisplayName("Shuffle").setSessionCommand(SessionCommand(CMD_SHUFFLE, Bundle.EMPTY)).build()
                MediaSessionAction.Repeat -> CommandButton.Builder(when (state.repeat) { RepeatMode.Off -> CommandButton.ICON_REPEAT_OFF; RepeatMode.All -> CommandButton.ICON_REPEAT_ALL; RepeatMode.One -> CommandButton.ICON_REPEAT_ONE })
                    .setDisplayName("Repeat").setSessionCommand(SessionCommand(CMD_REPEAT, Bundle.EMPTY)).build()
                MediaSessionAction.Rate -> CommandButton.Builder(if ((meta?.rating ?: 0u) > 0u) CommandButton.ICON_STAR_FILLED else CommandButton.ICON_STAR_UNFILLED)
                    .setDisplayName("Rate").setSessionCommand(SessionCommand(CMD_RATE, Bundle().apply { putInt(EXTRA_RATING, if ((meta?.rating ?: 0u) >= 5u) 0 else 5) })).setEnabled(meta != null).build()
                else -> null // transport actions are player commands
            }
        }
    }

    fun release() {
        session.release()
        player.release()
    }
}
