package app.hocket.ui.screens

import app.hocket.ui.screens.detail.firstSongIndex
import org.junit.Assert.assertEquals
import org.junit.Test

/** Where the album preview opens when nothing from the album is playing: past a short intro. */
class AlbumPreviewStartTest {
    private fun start(vararg songs: Pair<String, Int>) = firstSongIndex(songs.map { it.first to it.second * 1000L })

    @Test
    fun opensOnTheFirstSongWhenThereIsNoIntro() {
        assertEquals(0, start("Holocene" to 337, "Towers" to 188, "Michicant" to 210, "Hinnom, TX" to 166))
    }

    @Test
    fun skipsAShortSongTitledLikeAnIntro() {
        assertEquals(1, start("Intro" to 45, "Song" to 210, "Song 2" to 200))
        assertEquals(1, start("Prelude (feat. Someone)" to 100, "Song" to 210, "Song 2" to 200))
        assertEquals(1, start("The Ride (Intro)" to 70, "Song" to 210))
        assertEquals(1, start("SKIT" to 30, "Song" to 210))
    }

    @Test
    fun keepsALongSongCalledIntro() {
        assertEquals(0, start("Intro" to 300, "Song" to 210, "Song 2" to 200))
        assertEquals(0, start("Overture" to 420, "Act I" to 600))
    }

    @Test
    fun aWordOnlyCountsWhole() {
        assertEquals(0, start("Introvert" to 60, "Song" to 210))
    }

    @Test
    fun skipsAnUntitledOpenerFarShorterThanTheRest() {
        assertEquals(1, start("Wake" to 38, "Song" to 210, "Song 2" to 240, "Song 3" to 200))
    }

    @Test
    fun keepsTheFirstOfAnAlbumOfShortSongs() {
        assertEquals(0, start("Fast" to 55, "Faster" to 70, "Fastest" to 62, "Done" to 80))
    }

    @Test
    fun keepsTheFirstOfAShortRelease() {
        assertEquals(0, start("Wake" to 38, "Song" to 210, "Song 2" to 240))
    }

    @Test
    fun skipsAtMostTwoOpeners() {
        assertEquals(1, start("Intro" to 40, "Song" to 210, "Interlude" to 30))
        assertEquals(2, start("Intro" to 40, "Skit" to 20, "Song" to 210, "Song 2" to 200))
        assertEquals(2, start("Intro" to 40, "Skit" to 20, "Interlude" to 30, "Song" to 210))
    }

    @Test
    fun neverSkipsEverySong() {
        assertEquals(0, start("Intro" to 40))
        assertEquals(1, start("Intro" to 40, "Outro Interlude" to 30))
        assertEquals(0, firstSongIndex(emptyList()))
    }

    @Test
    fun anUnknownLengthStillSkipsByTitle() {
        assertEquals(1, start("Intro" to 0, "Song" to 0))
        assertEquals(0, start("Wake" to 0, "Song" to 210, "Song 2" to 240, "Song 3" to 200))
    }
}
