package app.hocket.ui.components

import app.hocket.core.MIXED_SOURCE
import app.hocket.core.api.AccountInfo
import app.hocket.core.api.AccountKind
import app.hocket.core.api.ProviderCapabilities
import app.hocket.core.api.ServerCapabilities
import app.hocket.core.api.TrackRef
import org.junit.Assert.assertEquals
import org.junit.Test

/** The source filter chip and the New playlist dialog's default account (docs/providers.md, Phase 3). */
class SourceFilterTest {
    private fun account(id: String, name: String) = AccountInfo(
        id = id, kind = AccountKind.Navidrome, priority = 0u, source = id, url = "https://$id/", username = "u", name = name,
        features = ProviderCapabilities(true, true, true, true, true, true, true, true, true, true, true, true, true, true, false, true),
        capabilities = ServerCapabilities(openSubsonic = false, extensions = emptyList(), transcodeOffset = false, formPost = false, songLyrics = false,
            sonicSimilarity = false, apiKeyAuthentication = false, transcodingExtension = false, nativeApi = false, meetsFloor = true),
        reachable = true,
    )

    @Test
    fun theChipOnlyShowsWithASecondAccountAndPlaylistsOfferMixed() {
        assertEquals(emptyList<SourceOption>(), SourceFilter.options(listOf(account("a", "Home")), mixed = true))
        val two = listOf(account("a", "Home"), account("b", ""))
        assertEquals(listOf("", "a", "b"), SourceFilter.options(two, mixed = false).map { it.id })
        assertEquals(listOf("", "a", "b", MIXED_SOURCE), SourceFilter.options(two, mixed = true).map { it.id })
        assertEquals("https://b/", SourceFilter.options(two, mixed = false)[2].label)
        // A choice the list doesn't offer (Mixed, outside playlists; a removed account) reads as All.
        assertEquals("", SourceFilter.current(MIXED_SOURCE, SourceFilter.options(two, mixed = false)))
        assertEquals("b", SourceFilter.current("b", SourceFilter.options(two, mixed = false)))
        assertEquals("", SourceFilter.current("gone", SourceFilter.options(two, mixed = false)))
    }

    @Test
    fun aNewPlaylistGoesWhereItsSongsAre() {
        val accounts = listOf("a", "b")
        assertEquals("b", NewPlaylistChoice.initial(accounts, listOf(TrackRef("b", "1"), TrackRef("b", "2"))))
        assertEquals(MIXED_SOURCE, NewPlaylistChoice.initial(accounts, listOf(TrackRef("a", "1"), TrackRef("b", "2"))))
        assertEquals("a", NewPlaylistChoice.initial(accounts, emptyList()))
        assertEquals("a", NewPlaylistChoice.initial(listOf("a"), listOf(TrackRef("a", "1"), TrackRef("x", "2"))))
    }
}
