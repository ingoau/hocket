package app.hocket.core

import app.hocket.core.api.ArtworkLayout
import app.hocket.core.api.ArtworkLayoutRequest

/**
 * The core's immersive-artwork classifier (`hocket_core::artwork`): how the full player shows a
 * cover. A pure function over the FFI, not a core query: the pixels never cross as JSON. Null when
 * the native library is not loaded (the fake core in debug builds and JVM tests): the player then
 * shows the artwork as a card.
 */
object ArtworkLayouts {
    /** [rgba] is [width]×[height] tightly packed RGBA8. Call off the main thread. */
    fun layout(rgba: ByteArray, width: Int, height: Int, request: ArtworkLayoutRequest): ArtworkLayout? {
        if (!NativeCore.isAvailable()) return null
        return try {
            val out = app.hocket.core.ffi.artworkLayout(rgba, width.toUInt(), height.toUInt(), HocketJson.json.encodeToString(ArtworkLayoutRequest.serializer(), request))
            HocketJson.json.decodeFromString(ArtworkLayout.serializer(), out)
        } catch (e: Exception) {
            null
        }
    }
}
