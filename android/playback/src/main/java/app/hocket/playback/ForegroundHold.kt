package app.hocket.playback

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Context
import android.content.pm.ServiceInfo
import android.os.Build
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/**
 * Keeps the `startForegroundService` contract for a service started by a media key (Media3's
 * `MediaButtonReceiver`, the widget's play button). Media3 only goes foreground once something
 * plays; when the resumed session does not play (nothing to resume, offline with nothing cached)
 * the system would kill the app for never calling `startForeground`. So the service goes foreground
 * at once with a silent, deferred notification (as Media3's own `stopSelfSafely` does) and lets go
 * of it after [HOLD_MS], or as soon as Media3's playback notification has taken over.
 */
class ForegroundHold(
    private val service: Service,
    private val scope: CoroutineScope,
    /** Media3's `isPlaybackOngoing`: its own notification holds the foreground. */
    private val playbackOngoing: () -> Boolean,
) {
    companion object {
        private const val TAG = "ForegroundHold"
        private const val CHANNEL = "hocket-starting"
        private const val NOTIFICATION_ID = 0x40c3e7
        const val HOLD_MS = 15_000L
        private const val POLL_MS = 250L
    }

    private var job: Job? = null

    /** Main thread. A media key started the service: hold the foreground unless playback already does. */
    fun hold() {
        if (playbackOngoing()) return
        val manager = service.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            manager.createNotificationChannel(NotificationChannel(CHANNEL, service.getString(R.string.foreground_hold_channel), NotificationManager.IMPORTANCE_LOW))
        }
        val notification = NotificationCompat.Builder(service, CHANNEL)
            .setSmallIcon(androidx.media3.session.R.drawable.media3_notification_small_icon)
            .setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_DEFERRED)
            .setVisibility(NotificationCompat.VISIBILITY_SECRET)
            .setOnlyAlertOnce(true)
            .build()
        try {
            ServiceCompat.startForeground(service, NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK)
        } catch (e: IllegalStateException) {
            // Not allowed from here (a plain start in the background): nothing was promised either.
            Log.w(TAG, "could not hold the foreground: ${e.message}")
            return
        }
        job?.cancel()
        job = scope.launch {
            var waited = 0L
            while (!playbackOngoing() && waited < HOLD_MS) { delay(POLL_MS); waited += POLL_MS }
            // Media3's notification is the foreground one now: just drop ours. Otherwise let go of
            // the foreground; the service's idle stop takes it from there.
            if (playbackOngoing()) manager.cancel(NOTIFICATION_ID)
            else ServiceCompat.stopForeground(service, ServiceCompat.STOP_FOREGROUND_REMOVE)
        }
    }

    fun release() { job?.cancel() }
}
