package app.hocket.diagnostics

import android.content.Context
import android.hardware.Sensor
import android.hardware.SensorEvent
import android.hardware.SensorEventListener
import android.hardware.SensorManager
import android.view.KeyEvent
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlin.math.sqrt

/**
 * What opens developer diagnostics while developer mode is on: shaking the phone, or Ctrl+Shift+D on
 * a hardware keyboard (the desktop app's shortcut). The activity forwards key presses with
 * [onKeyDown]; [DeveloperTriggerEffect] listens for both while the UI is resumed.
 */
object DeveloperTriggers {
    private val _requests = MutableSharedFlow<Unit>(extraBufferCapacity = 1)
    val requests: SharedFlow<Unit> = _requests.asSharedFlow()

    /**
     * From the activity's `dispatchKeyEvent`: true when it was the shortcut and something is
     * listening (developer mode is on), which consumes it; otherwise the key goes on as usual.
     */
    fun onKeyDown(event: KeyEvent): Boolean {
        if (event.action != KeyEvent.ACTION_DOWN || event.repeatCount != 0) return false
        if (event.keyCode != KeyEvent.KEYCODE_D || !event.isCtrlPressed || !event.isShiftPressed || event.isAltPressed) return false
        return _requests.subscriptionCount.value > 0 && _requests.tryEmit(Unit)
    }

    fun request() {
        _requests.tryEmit(Unit)
    }
}

/**
 * Shake detection on the accelerometer: [SHAKES] jolts over [THRESHOLD_G] within [WINDOW_MS], then
 * quiet for [COOLDOWN_MS] so one long shake opens the page once. Pure over its inputs (tests).
 */
class ShakeDetector(private val onShake: () -> Unit) : SensorEventListener {
    private val jolts = ArrayDeque<Long>()
    private var lastShakeAt = Long.MIN_VALUE / 2

    override fun onSensorChanged(event: SensorEvent) {
        if (event.values.size >= 3) onAcceleration(event.values[0], event.values[1], event.values[2], event.timestamp / 1_000_000)
    }

    override fun onAccuracyChanged(sensor: Sensor?, accuracy: Int) = Unit

    /** One reading in m/s², at [atMs]. */
    fun onAcceleration(x: Float, y: Float, z: Float, atMs: Long) {
        val g = sqrt(x * x + y * y + z * z) / SensorManager.GRAVITY_EARTH
        if (g < THRESHOLD_G || atMs - lastShakeAt < COOLDOWN_MS) return
        // One jolt per JOLT_GAP_MS: a single swing reads above the threshold for several samples.
        if (jolts.isNotEmpty() && atMs - jolts.last() < JOLT_GAP_MS) return
        jolts.addLast(atMs)
        while (jolts.isNotEmpty() && atMs - jolts.first() > WINDOW_MS) jolts.removeFirst()
        if (jolts.size >= SHAKES) {
            jolts.clear()
            lastShakeAt = atMs
            onShake()
        }
    }

    companion object {
        const val THRESHOLD_G = 2.5f
        const val SHAKES = 3
        const val WINDOW_MS = 1_200L
        const val JOLT_GAP_MS = 120L
        const val COOLDOWN_MS = 2_000L
    }
}

/**
 * While [enabled] and the UI is resumed: listens for a shake and for [DeveloperTriggers] requests,
 * and runs [open] for each. Nothing is registered while developer mode is off.
 */
@Composable
fun DeveloperTriggerEffect(enabled: Boolean, open: () -> Unit) {
    val currentOpen by rememberUpdatedState(open)
    if (!enabled) return
    LaunchedEffect(Unit) { DeveloperTriggers.requests.collect { currentOpen() } }
    val context = LocalContext.current
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    DisposableEffect(context, lifecycle) {
        val sensors = context.getSystemService(Context.SENSOR_SERVICE) as? SensorManager
        val accelerometer = sensors?.getDefaultSensor(Sensor.TYPE_ACCELEROMETER)
        val detector = ShakeDetector { DeveloperTriggers.request() }
        var registered = false
        fun register(on: Boolean) {
            if (sensors == null || accelerometer == null || on == registered) return
            registered = on
            if (on) sensors.registerListener(detector, accelerometer, SensorManager.SENSOR_DELAY_UI) else sensors.unregisterListener(detector)
        }
        val observer = LifecycleEventObserver { _, _ -> register(lifecycle.currentState.isAtLeast(Lifecycle.State.RESUMED)) }
        lifecycle.addObserver(observer)
        onDispose {
            lifecycle.removeObserver(observer)
            register(false)
        }
    }
}
