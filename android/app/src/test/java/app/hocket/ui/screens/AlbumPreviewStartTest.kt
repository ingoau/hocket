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
    fun keepsTheFirstWhenTheWordNamesThePieces() {
        // Chopin's Preludes: every song is one.
        assertEquals(0, start("Prelude No. 1" to 40, "Prelude No. 2" to 130, "Prelude No. 3" to 60, "Prelude No. 4" to 120))
        // The Well-Tempered Clavier: Prelude and Fugue pairs.
        assertEquals(0, start("Prelude No. 1 in C" to 120, "Fugue No. 1 in C" to 110, "Prelude No. 2" to 90, "Fugue No. 2" to 100, "Prelude No. 3" to 80, "Fugue No. 3" to 130))
    }

    @Test
    fun keepsATitledOpenerNotMuchShorterThanTheRest() {
        // A cello suite: the Prelude is one of the movements.
        assertEquals(0, start("I. Prelude" to 140, "II. Allemande" to 240, "III. Courante" to 150, "IV. Sarabande" to 200, "V. Menuets" to 180, "VI. Gigue" to 100))
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
