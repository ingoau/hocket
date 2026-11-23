package app.hocket.jobs

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
import app.hocket.core.api.Job
import app.hocket.playback.BackgroundJobs
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
 * Keeps downloads and syncs ([BackgroundJobs]) going once the app leaves the screen: a `dataSync`
 * foreground service with an ongoing progress notification, bound to the playback service so the
 * core (which runs the jobs) is neither stopped as idle nor frozen with a cached process. Media3
 * owns the playback service's own foreground state, so this stays a separate service. Started by
 * [HocketApp] when such a job starts; stops itself, taking the notification with it, once there are
 * none (the playback service's idle stop then takes over). Without the foreground (refused from the
 * background) the jobs still run while the app is alive.
 */
class JobsService : Service() {
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
                ServiceCompat.startForeground(this, NOTIFICATION_ID, notification(emptyList()), if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC else 0)
                true
            } catch (e: Exception) {
                // ForegroundServiceStartNotAllowedException (API 31+) and the like: the jobs carry on while the app lives.
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
            BackgroundJobs.activeFlow(app.client.flatMapLatest { it?.jobs ?: flowOf(emptyList()) }).conflate().collect { active ->
                if (active.isEmpty()) {
                    finish()
                    return@collect
                }
                val manager = NotificationManagerCompat.from(this@JobsService)
                // Without the permission (API 33+) the system shows the service's own entry instead.
                if (foreground && manager.areNotificationsEnabled()) {
                    try { manager.notify(NOTIFICATION_ID, notification(active)) } catch (e: SecurityException) { Log.w(TAG, "couldn't update the notification") }
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

    /** The dataSync time limit (API 35+): let go of the foreground; the jobs go on while the app lives. */
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

    private fun notification(active: List<Job>): android.app.Notification {
        val manager = getSystemService(NotificationManager::class.java)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            manager.createNotificationChannel(NotificationChannel(CHANNEL, getString(R.string.jobs_notification_channel), NotificationManager.IMPORTANCE_LOW))
        }
        val open = PendingIntent.getActivity(
            this, 0,
            Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val progress = BackgroundJobs.progress(active)
        val title = when (active.size) {
            0 -> getString(R.string.jobs_notification_starting)
            1 -> active.single().label
            else -> resources.getQuantityString(R.plurals.jobs_notification_many, active.size, active.size)
        }
        return NotificationCompat.Builder(this, CHANNEL)
            .setSmallIcon(R.drawable.media3_notification_small_icon)
            .setContentTitle(title)
            .apply { if (progress != null) setContentText(getString(R.string.jobs_progress, progress.first, progress.second)) }
            .setProgress(progress?.second ?: 0, progress?.first ?: 0, progress == null)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setSilent(true)
            .setCategory(NotificationCompat.CATEGORY_PROGRESS)
            .setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_DEFERRED)
            .setContentIntent(open)
            .build()
    }

    companion object {
        private const val TAG = "JobsService"
        const val CHANNEL = "jobs"
        private const val NOTIFICATION_ID = 0x70b5
        private const val UPDATE_MS = 1_000L

        /** Starts the service while a download or sync runs; refused from the background, which is fine. */
        fun start(context: Context) {
            // A plain start, not startForegroundService: jobs start from the app's own screen,
            // where the service may go foreground; if that fails there, stopping is safe.
            try {
                context.startService(Intent(context, JobsService::class.java))
            } catch (e: IllegalStateException) {
                // Not allowed from the background (a sync the core started with no app on screen).
                Log.w(TAG, "couldn't start the jobs service (${e.javaClass.simpleName}); the jobs run while the app does")
            } catch (e: SecurityException) {
                Log.w(TAG, "couldn't start the jobs service: ${e.message}")
            }
        }
    }
}
