package app.hocket.ui.components

import app.hocket.core.ArtworkSizes
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Offline the core answers with whichever size it kept, and a display ask with one a little larger;
 * only a file that shows the asked size as well as its own is remembered.
 */
class ArtworkExactTest {
    @Test
    fun theAskedSizesOwnFileIsExact() {
        assertTrue(isExactArtwork("/c/images/srv/al-1_640.img", ArtworkSizes.FULL))
        assertTrue(isExactArtwork("/c/images/srv/al-1_1280.img", ArtworkSizes.HERO))
        assertTrue(isExactArtwork("/c/images/srv/al-1_64.img", ArtworkSizes.THUMB))
    }

    @Test
    fun aLittleLargerFileIsExactButNotItsOwn() {
        assertTrue(isExactArtwork("/c/images/srv/al-1_640.img", ArtworkSizes.LIST))
        assertTrue(isExactArtwork("/c/images/srv/al-1_160.img", ArtworkSizes.THUMB))
        assertFalse(isOwnArtwork("/c/images/srv/al-1_640.img", ArtworkSizes.LIST))
        assertTrue(isOwnArtwork("/c/images/srv/al-1_640.img", ArtworkSizes.FULL))
    }

    @Test
    fun anotherSizeStandingInIsNot() {
        assertFalse(isExactArtwork("/c/images/srv/al-1_160.img", ArtworkSizes.FULL))
        assertFalse(isExactArtwork("/c/images/srv/al-1_1280.img", ArtworkSizes.LIST))
        assertFalse(isExactArtwork("/c/images/srv/al-1_320.img", ArtworkSizes.THUMB))
    }

    @Test
    fun pathsOfAnotherShapeCountAsExact() {
        assertTrue(isExactArtwork("/tmp/cover.png", ArtworkSizes.GRID))
    }
}
