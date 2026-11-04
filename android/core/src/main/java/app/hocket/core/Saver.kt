package app.hocket.core

import app.hocket.core.api.BatterySaver
import app.hocket.core.api.SaverFeature
import app.hocket.core.api.SaverFeature.*
import app.hocket.core.api.SaverLevel
import kotlinx.serialization.json.Json

/**
 * Battery saver levels as the core defines them (`crates/hocket-core/src/saver.rs`): what each
 * preset turns off, and which level is in force. The core resolves it ([BatterySaver]); this
 * mirror serves the settings screen (the switches a level stands for) and the fake core.
 */
object Saver {
    private val LIGHT = listOf(StillBackground, StillSeekWave, LighterLyrics)
    private val MEDIUM = LIGHT + listOf(RefreshCap, SlowSync)
    private val MAX = MEDIUM + listOf(SimpleLyrics, NoImmersiveArtwork, NoArtworkBlur, SmallArtwork)

    /** Every feature, in the order the settings screen lists them. */
    val ALL: List<SaverFeature> = MAX

    /** The presets the slider steps through, weakest first. */
    val PRESETS: List<SaverLevel> = listOf(SaverLevel.Off, SaverLevel.Light, SaverLevel.Medium, SaverLevel.Max)

    val OFF = BatterySaver(SaverLevel.Off, automatic = false, features = emptyList())

    fun preset(level: SaverLevel): List<SaverFeature> = when (level) {
        SaverLevel.Off, SaverLevel.Custom -> emptyList()
        SaverLevel.Light -> LIGHT
        SaverLevel.Medium -> MEDIUM
        SaverLevel.Max -> MAX
    }

    fun features(level: SaverLevel, custom: List<SaverFeature>): List<SaverFeature> =
        (if (level == SaverLevel.Custom) custom else preset(level)).distinct().sorted()

    private fun rank(level: SaverLevel): Int? = when (level) {
        SaverLevel.Off -> 0
        SaverLevel.Light -> 1
        SaverLevel.Medium -> 2
        SaverLevel.Max -> 3
        SaverLevel.Custom -> null
    }

    /** As the core: power save raises the chosen level to the automatic one, never lowers it. */
    fun resolve(chosen: SaverLevel, autoLevel: SaverLevel, autoEngage: Boolean, powerSave: Boolean, custom: List<SaverFeature>): BatterySaver {
        val c = rank(chosen)
        val a = rank(autoLevel)
        val auto = autoEngage && powerSave && autoLevel != SaverLevel.Off &&
            (c == 0 || (c != null && a != null && a > c))
        val level = if (auto) autoLevel else chosen
        return BatterySaver(level, auto, features(level, custom))
    }

    private val json = Json { ignoreUnknownKeys = true }

    /** A `battery.level` / `battery.autoLevel` value (JSON text). */
    fun parseLevel(value: String?): SaverLevel? =
        value?.let { runCatching { json.decodeFromString(SaverLevel.serializer(), it) }.getOrNull() }

    /** A `battery.custom` value (JSON text). */
    fun parseCustom(value: String?): List<SaverFeature> =
        value?.let { runCatching { json.decodeFromString(kotlinx.serialization.builtins.ListSerializer(SaverFeature.serializer()), it) }.getOrNull() }
            .orEmpty()

    fun levelValue(level: SaverLevel): String = "\"${level.string}\""

    fun customValue(features: Collection<SaverFeature>): String =
        features.distinct().sorted().joinToString(",", "[", "]") { "\"${it.string}\"" }
}

fun BatterySaver.has(feature: SaverFeature): Boolean = feature in features

val BatterySaver.engaged: Boolean get() = features.isNotEmpty()
