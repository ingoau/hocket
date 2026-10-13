package app.hocket.playback

import android.content.Context
import android.content.Intent
import androidx.media3.session.MediaButtonReceiver

/**
 * Media3's media button receiver (playback resumption from a stopped service), declining to start
 * the service when there is nothing to resume: the last session state the service saw had no track
 * (a fresh install, after sign-out). The service would only start to do nothing.
 */
class HocketMediaButtonReceiver : MediaButtonReceiver() {
    override fun shouldStartForegroundService(context: Context, intent: Intent): Boolean = WidgetState.load(context).hasTrack
}
