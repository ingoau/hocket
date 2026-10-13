package app.hocket.playback

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.BroadcastReceiver
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.util.Log
import android.view.KeyEvent
import android.view.View
import android.widget.RemoteViews
import androidx.core.content.ContextCompat
import app.hocket.core.Commands
import app.hocket.core.api.MediaSessionAction
import app.hocket.core.api.MediaSessionState
import java.io.File

/**
 * What the home-screen widget shows: the session's track and, while another Connect device plays it,
 * that device. Kept in SharedPreferences (excluded from backups) so a widget placed or redrawn while
 * the service is stopped shows the last state instead of a blank.
 */
data class WidgetState(
    val title: String? = null,
    val artist: String? = null,
    val artworkPath: String? = null,
    val playing: Boolean = false,
    /** The Connect device playing the session when it is not this one. */
    val remoteDevice: String? = null,
) {
    val hasTrack: Boolean get() = title != null

    companion object {
        const val PREFS = "hocket-widget"

        fun of(state: MediaSessionState, remoteDevice: String?): WidgetState {
            val meta = state.metadata ?: return WidgetState()
            return WidgetState(meta.title, meta.artist, meta.artworkPath, state.isPlaying, remoteDevice)
        }

        fun load(context: Context): WidgetState {
            val p = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            return WidgetState(p.getString("title", null), p.getString("artist", null), p.getString("artwork", null), p.getBoolean("playing", false), p.getString("device", null))
        }
    }

    fun save(context: Context) {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
            .putString("title", title).putString("artist", artist).putString("artwork", artworkPath)
            .putBoolean("playing", playing).putString("device", remoteDevice)
            .apply()
    }
}

/**
 * The now-playing home-screen widget: cover, title, artist, the Connect device playing it (when it is
 * not this one), and previous / play-pause / next. Tapping it opens the app.
 *
 * [PlaybackService] pushes every change through [update]; the provider itself only redraws the stored
 * state when the launcher asks. Buttons go to [WidgetActionReceiver] (not exported), which forwards
 * them to the running core, or starts the service for a play while it is stopped.
 */
class NowPlayingWidget : AppWidgetProvider() {
    companion object {
        private const val TAG = "NowPlayingWidget"
        /** Covers are decoded down to about this size: a widget bitmap travels over binder. */
        private const val ART_PX = 256
        private var last: WidgetState? = null

        /** Stores [state] and redraws every placed widget when it changed. Main thread. */
        fun update(context: Context, state: WidgetState) {
            if (state == last) return
            last = state
            state.save(context)
            val manager = AppWidgetManager.getInstance(context) ?: return
            val ids = manager.getAppWidgetIds(ComponentName(context, NowPlayingWidget::class.java))
            if (ids.isEmpty()) return
            runCatching { manager.updateAppWidget(ids, views(context, state)) }.onFailure { Log.w(TAG, "widget update failed: ${it.message}") }
        }

        fun views(context: Context, state: WidgetState): RemoteViews = RemoteViews(context.packageName, R.layout.widget_now_playing).apply {
            setTextViewText(R.id.widget_title, state.title ?: context.getString(R.string.widget_nothing_playing))
            setTextViewText(R.id.widget_artist, state.artist.orEmpty())
            setViewVisibility(R.id.widget_artist, if (state.artist.isNullOrBlank()) View.GONE else View.VISIBLE)
            val device = when {
                !state.hasTrack -> null
                state.remoteDevice != null -> context.getString(R.string.widget_playing_on, state.remoteDevice)
                else -> context.getString(R.string.widget_this_device)
            }
            setTextViewText(R.id.widget_device, device.orEmpty())
            setViewVisibility(R.id.widget_device, if (device == null) View.GONE else View.VISIBLE)
            val art = state.artworkPath?.let(::decode)
            if (art != null) setImageViewBitmap(R.id.widget_art, art) else setImageViewResource(R.id.widget_art, R.drawable.widget_art_placeholder)
            setImageViewResource(R.id.widget_play, if (state.playing) R.drawable.widget_pause else R.drawable.widget_play)
            setContentDescription(R.id.widget_play, context.getString(if (state.playing) R.string.widget_pause else R.string.widget_play))
            setOnClickPendingIntent(R.id.widget_play, WidgetActionReceiver.intent(context, WidgetActionReceiver.ACTION_PLAY_PAUSE))
            setOnClickPendingIntent(R.id.widget_next, WidgetActionReceiver.intent(context, WidgetActionReceiver.ACTION_NEXT))
            setOnClickPendingIntent(R.id.widget_previous, WidgetActionReceiver.intent(context, WidgetActionReceiver.ACTION_PREVIOUS))
            val buttons = if (state.hasTrack) View.VISIBLE else View.GONE
            setViewVisibility(R.id.widget_next, buttons)
            setViewVisibility(R.id.widget_previous, buttons)
            context.packageManager.getLaunchIntentForPackage(context.packageName)?.let { launch ->
                setOnClickPendingIntent(R.id.widget_root, PendingIntent.getActivity(context, 0, launch, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT))
            }
        }

        /** The cached cover at [path], sampled down to about [ART_PX]; null when it is gone. */
        private fun decode(path: String): Bitmap? = runCatching {
            val file = File(path.removePrefix("file://")).takeIf { it.isFile } ?: return null
            val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
            BitmapFactory.decodeFile(file.path, bounds)
            var sample = 1
            while (bounds.outWidth / (sample * 2) >= ART_PX && bounds.outHeight / (sample * 2) >= ART_PX) sample *= 2
            BitmapFactory.decodeFile(file.path, BitmapFactory.Options().apply { inSampleSize = sample })
        }.getOrNull()
    }

    override fun onUpdate(context: Context, appWidgetManager: AppWidgetManager, appWidgetIds: IntArray) {
        appWidgetManager.updateAppWidget(appWidgetIds, views(context, last ?: WidgetState.load(context)))
    }
}

/**
 * The widget's buttons. Not exported: only the widget's own PendingIntents reach it. With the
 * service running, the press is the matching session command to the core (which forwards it when
 * another device plays). With it stopped, only play means anything: the service is started with a
 * play key, as Media3's `MediaButtonReceiver` would, and resumes through playback resumption. A
 * widget press lets the app start a foreground service from the background.
 */
class WidgetActionReceiver : BroadcastReceiver() {
    companion object {
        private const val TAG = "WidgetActionReceiver"
        const val ACTION_PLAY_PAUSE = "app.hocket.playback.widget.PLAY_PAUSE"
        const val ACTION_NEXT = "app.hocket.playback.widget.NEXT"
        const val ACTION_PREVIOUS = "app.hocket.playback.widget.PREVIOUS"

        fun intent(context: Context, action: String): PendingIntent =
            PendingIntent.getBroadcast(context, action.hashCode(), Intent(context, WidgetActionReceiver::class.java).setAction(action), PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)

        /** The session command for [action] given the widget's [state]; null when there is none. */
        fun command(action: String?, state: WidgetState): MediaSessionAction? = when (action) {
            ACTION_PLAY_PAUSE -> if (state.playing) MediaSessionAction.Pause else MediaSessionAction.Play
            ACTION_NEXT -> MediaSessionAction.Next
            ACTION_PREVIOUS -> MediaSessionAction.Previous
            else -> null
        }
    }

    override fun onReceive(context: Context, intent: Intent) {
        val action = command(intent.action, WidgetState.load(context)) ?: return
        val core = CoreHost.current
        if (core != null) {
            core.dispatch(Commands.mediaSessionCommand(action))
            return
        }
        if (action != MediaSessionAction.Play) return
        val start = Intent(Intent.ACTION_MEDIA_BUTTON)
            .setClass(context, PlaybackService::class.java)
            .putExtra(Intent.EXTRA_KEY_EVENT, KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_MEDIA_PLAY))
        try {
            ContextCompat.startForegroundService(context, start)
        } catch (e: IllegalStateException) {
            Log.w(TAG, "could not start playback from the widget: ${e.message}")
        }
    }
}
