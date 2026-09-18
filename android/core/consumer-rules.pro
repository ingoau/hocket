# JNA / UniFFI: keep the native bridge intact under R8.
-keep class com.sun.jna.** { *; }
-keep class app.hocket.core.ffi.** { *; }
-dontwarn java.awt.*
