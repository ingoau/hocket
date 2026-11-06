package app.hocket

/** Casting through the Google Cast SDK (this build includes `:cast`). */
internal object CastSetup {
    fun install() = app.hocket.cast.HocketCast.install()
}
