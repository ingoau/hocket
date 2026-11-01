package app.hocket.diagnostics

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.app.ServiceCompat
import app.hocket.HocketApp
import app.hocket.MainActivity
import app.hocket.R
import app.hocket.core.api.ReportUploadState
import app.hocket.core.api.ReportUploadStatus
import app.hocket.playback.PlaybackServiceConnection
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.conflate
import kotlinx.coroutines.flow.flatMapLatest
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.launch

/**
 * Keeps a "Save to Coordinator" upload going once the app leaves the screen (`docs/reports.md`):
 * a `dataSync` foreground service with an ongoing progress notification, bound to the playback
 * service so the core (which does the upload) is not stopped as idle. Started by [HocketApp] when
 * the upload becomes Uploading; stops itself, taking the notification with it, as soon as it is
 * anything else (the in-app bar says how it ended). Without the foreground (refused from the
 * background) the upload still runs while the app is alive.
 */
class ReportUploadService : Service() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private var bound = false
    private var foreground = false
    private var following: kotlinx.coroutines.Job? = null

    /** Holds the playback service, and with it the core: its idle stop only counts unbound apps. */
    private val coreConnection = object : ServiceConnection {
        override fun onServiceConnected(name: ComponentName?, service: IBinder?) = Unit
        override fun onServiceDisconnected(name: ComponentName?) = Unit
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        bound = runCatching { bindService(PlaybackServiceConnection.bindIntent(this), coreConnection, Context.BIND_AUTO_CREATE) }.getOrDefault(false)
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (!foreground) {
            foreground = try {
                ServiceCompat.startForeground(this, NOTIFICATION_ID, notification(null), if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC else 0)
                true
            } catch (e: Exception) {
                // ForegroundServiceStartNotAllowedException (API 31+) and the like: the upload carries on while the app lives.
                Log.w(TAG, "couldn't go foreground: ${e.javaClass.simpleName}")
                stopSelf()
                return START_NOT_STICKY
            }
        }
        if (following == null) following = follow()
        return START_NOT_STICKY
    }

    @OptIn(ExperimentalCoroutinesApi::class)
    private fun follow(): kotlinx.coroutines.Job? {
        val app = application as? HocketApp ?: run { finish(); return null }
        return scope.launch {
            app.client.flatMapLatest { it?.reportUpload ?: flowOf(null) }.conflate().collect { status ->
                if (status?.state != ReportUploadState.Uploading) {
                    finish()
                    return@collect
                }
                val manager = NotificationManagerCompat.from(this@ReportUploadService)
                // Without the permission (API 33+) the system shows the service's own entry instead.
                if (foreground && manager.areNotificationsEnabled()) {
                    try { manager.notify(NOTIFICATION_ID, notification(status)) } catch (e: SecurityException) { Log.w(TAG, "couldn't update the notification") }
                }
                // At most about once a second: the core reports far more often than that.
                delay(UPDATE_MS)
            }
        }
    }

    private fun finish() {
        if (foreground) ServiceCompat.stopForeground(this, ServiceCompat.STOP_FOREGROUND_REMOVE)
        foreground = false
        stopSelf()
    }

    /** The dataSync time limit (API 35+): let go of the foreground; the upload goes on while the app lives. */
    override fun onTimeout(startId: Int, fgsType: Int) {
        Log.w(TAG, "foreground time limit reached")
        finish()
    }

    override fun onDestroy() {
        scope.cancel()
        if (bound) runCatching { unbindService(coreConnection) }
        bound = false
        super.onDestroy()
    }

    private fun notification(status: ReportUploadStatus?): android.app.Notification {
        val manager = getSystemService(NotificationManager::class.java)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            manager.createNotificationChannel(NotificationChannel(CHANNEL, getString(R.string.report_upload_channel), NotificationManager.IMPORTANCE_LOW))
        }
        val open = PendingIntent.getActivity(
            this, 0,
            Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val percent = status?.let { percent(it) }
        return NotificationCompat.Builder(this, CHANNEL)
            .setSmallIcon(R.drawable.media3_notification_small_icon)
            .setContentTitle(if (percent != null) getString(R.string.report_upload_progress, percent) else getString(R.string.report_upload_starting))
            .setContentText(getString(R.string.report_upload_notification_text))
            .setProgress(100, percent ?: 0, percent == null)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setSilent(true)
            .setCategory(NotificationCompat.CATEGORY_PROGRESS)
            .setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_IMMEDIATE)
            .setContentIntent(open)
            .build()
    }

    companion object {
        private const val TAG = "ReportUploadService"
        const val CHANNEL = "report-upload"
        private const val NOTIFICATION_ID = 0x7e9047
        private const val UPDATE_MS = 1_000L

        /** 0–100, or null while the size is not known yet. */
        fun percent(status: ReportUploadStatus): Int? =
            if (status.totalBytes <= 0.0) null else (status.sentBytes * 100 / status.totalBytes).toInt().coerceIn(0, 100)

        /** Starts the service for an upload that is Uploading; refused from the background, which is fine. */
        fun start(context: Context) {
            // A plain start, not startForegroundService: the upload starts from the app's own
            // screen, where the service may go foreground; and if going foreground fails
            // there, stopping is safe (no startForegroundService promise to break).
            try {
                context.startService(Intent(context, ReportUploadService::class.java))
            } catch (e: IllegalStateException) {
                // Not allowed from the background (a resumed upload with no app on screen).
                Log.w(TAG, "couldn't start the upload service (${e.javaClass.simpleName}); the upload runs while the app does")
            } catch (e: SecurityException) {
                Log.w(TAG, "couldn't start the upload service: ${e.message}")
            }
        }
    }
}
