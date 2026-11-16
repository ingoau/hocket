package app.hocket

import android.content.Context

/** A FOSS build has no in-app scanner: the system camera opens a setup code's hocket://setup link. */
internal object SetupScanner {
    const val available = false

    fun scan(context: Context, onResult: (String) -> Unit, onError: () -> Unit) = Unit
}
