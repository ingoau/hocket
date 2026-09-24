package app.hocket.playback

import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner

/**
 * Binds the app to [PlaybackService] while any UI is started and unbinds when none is (register on
 * `ProcessLifecycleOwner`). Binding from `Application.onCreate` never let go, so the service could
 * never reach its idle stop and the process lived on after the task was swiped away. With this, the
 * only binders are visible UI; playback keeps the service alive on its own (Media3 foreground), and
 * an idle service stops itself once the UI is gone.
 */
class ForegroundBinder(private val connection: PlaybackServiceConnection) : DefaultLifecycleObserver {
    override fun onStart(owner: LifecycleOwner) = connection.bind()
    override fun onStop(owner: LifecycleOwner) = connection.unbind()
}
