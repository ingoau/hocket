package app.hocket.ui.components

import app.hocket.core.ArtworkSizes
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** Offline the core answers with whichever size it kept; only the asked size's own file is remembered. */
class ArtworkExactTest {
    @Test
    fun theAskedSizesOwnFileIsExact() {
        assertTrue(isExactArtwork("/c/images/srv/al-1_640.img", ArtworkSizes.FULL))
        assertTrue(isExactArtwork("/c/images/srv/al-1_1280.img", ArtworkSizes.HERO))
        assertTrue(isExactArtwork("/c/images/srv/al-1_64.img", ArtworkSizes.THUMB))
    }

    @Test
    fun anotherSizeStandingInIsNot() {
        assertFalse(isExactArtwork("/c/images/srv/al-1_160.img", ArtworkSizes.FULL))
        assertFalse(isExactArtwork("/c/images/srv/al-1_640.img", ArtworkSizes.LIST))
    }

    @Test
    fun pathsOfAnotherShapeCountAsExact() {
        assertTrue(isExactArtwork("/tmp/cover.png", ArtworkSizes.GRID))
    }
}
