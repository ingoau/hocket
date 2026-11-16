package app.hocket.ui.components

import androidx.compose.material3.SnackbarHostState

/**
 * Where the app's snackbars go while a modal sheet covers the shell's own host (a sheet is a window
 * of its own, so a snackbar behind its scrim can be seen but not tapped: an Undo out of reach). The
 * sheet sets [host] to one inside it while it is open; the toast collector shows there instead.
 */
object SnackbarRedirect {
    @Volatile
    var host: SnackbarHostState? = null
}
