package app.hocket.playback

import androidx.media3.common.HeartRating
import androidx.media3.common.PercentageRating
import androidx.media3.common.StarRating
import androidx.media3.common.ThumbRating
import app.hocket.core.Commands
import org.junit.Assert.assertEquals
import org.junit.Test

/** A controller's standard rating as the core command it becomes. */
class MediaSessionRatingTest {
    private fun command(rating: androidx.media3.common.Rating) = MediaSessionBridge.ratingCommand("t1", rating)

    @Test
    fun heartsAndThumbsLoveOrUnlove() {
        assertEquals(Commands.loveTrack("t1", true), command(HeartRating(true)))
        assertEquals(Commands.loveTrack("t1", false), command(HeartRating(false)))
        assertEquals(Commands.loveTrack("t1", false), command(HeartRating()))
        assertEquals(Commands.loveTrack("t1", true), command(ThumbRating(true)))
        assertEquals(Commands.loveTrack("t1", false), command(ThumbRating(false)))
    }

    @Test
    fun starsAndPercentagesRateOutOfFive() {
        assertEquals(Commands.rateTrack("t1", 4), command(StarRating(5, 4f)))
        assertEquals(Commands.rateTrack("t1", 5), command(StarRating(3, 3f)))
        assertEquals(Commands.rateTrack("t1", 0), command(StarRating(5)))
        assertEquals(Commands.rateTrack("t1", 3), command(PercentageRating(60f)))
        assertEquals(Commands.rateTrack("t1", 0), command(PercentageRating()))
    }
}
