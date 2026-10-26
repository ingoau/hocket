package app.hocket.ui.components

import app.hocket.core.ActionIds
import app.hocket.core.Commands
import app.hocket.core.api.ActionDescriptor
import app.hocket.core.api.OfflineState
import app.hocket.core.api.Playlist
import app.hocket.core.api.PlaylistEntryRef
import app.hocket.core.api.PlaylistMembership
import app.hocket.share.geniusSearch
import app.hocket.share.shareChoices
import app.hocket.share.songLinkSearch
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** The menus' and the add to playlist sheet's pure rules: placement, grouping, ticks, Done's commands and words. */
class MenuLogicTest {
    private fun d(id: String, enabled: Boolean = true, destructive: Boolean = false) = ActionDescriptor(id, id, "x", "c", enabled, null, true, destructive)

    private fun playlist(id: String, name: String, songs: Int = 10, changed: Double? = null, smart: Boolean = false, mine: Boolean = true) =
        Playlist(id, "s", name, null, "me", false, songs.toUInt(), 0u, null, null, changed, smart, mine, OfflineState.None)

    private val songActions = listOf(
        ActionIds.PIN_TO_LIBRARY, ActionIds.UNPIN_FROM_LIBRARY, ActionIds.PLAY, ActionIds.PLAY_SHUFFLED, ActionIds.PLAY_NEXT, ActionIds.PLAY_LATER,
        ActionIds.ADD_TO_PLAYLIST, ActionIds.LOVE, ActionIds.UNLOVE, ActionIds.rate(5), ActionIds.rate(0), ActionIds.DOWNLOAD, ActionIds.UNPIN,
        ActionIds.GO_TO_ALBUM, ActionIds.GO_TO_ARTIST, ActionIds.REMOVE_FROM_PLAYLIST, ActionIds.SLEEP_TIMER, "somethingNew",
    ).map { d(it, enabled = it != ActionIds.UNPIN_FROM_LIBRARY) }

    @Test
    fun theSongMenuPlacesTheRegistryPerTheDesign() {
        val l = menuLayout(songActions + d(ActionIds.REMOVE_FROM_QUEUE, enabled = false), MenuKind.Song, canGoToAlbum = true, canGoToArtist = true, hasInfo = true)
        assertEquals(listOf(ActionIds.PLAY, ActionIds.PLAY_SHUFFLED, ActionIds.PLAY_NEXT, ActionIds.PLAY_LATER), l.play.map { it.id })
        assertTrue(l.love != null && l.rating)
        assertEquals(listOf(ActionIds.ADD_TO_PLAYLIST, "download", "share", "genius", ActionIds.GO_TO_ALBUM), l.primary.map { it.key })
        // Unplaced registry actions are not lost: they follow under More (ui-only ones it can't perform are not).
        assertEquals(listOf("pin", ActionIds.GO_TO_ARTIST, "info", "somethingNew"), l.more.map { it.key })
        // Context-only actions show where the core enables them.
        assertEquals(listOf(ActionIds.REMOVE_FROM_PLAYLIST), l.destructive.map { it.key })
        assertEquals(5, l.hidden)
    }

    @Test
    fun anExtraStandsInForTheRegistryActionItReplacesAndExtrasSitInPlace() {
        val remove = SongMenuExtra("Remove from Late night", app.hocket.ui.icons.HocketIcons.Filled.Remove, {}, destructive = true, testTag = "rm", replaces = ActionIds.REMOVE_FROM_PLAYLIST)
        val sleep = SongMenuExtra("Sleep timer", app.hocket.ui.icons.HocketIcons.Filled.Bedtime, {}, testTag = "sleep")
        val l = menuLayout(songActions, MenuKind.Song, listOf(remove, sleep))
        assertEquals(listOf(ActionIds.ADD_TO_PLAYLIST, "download", "share", "genius", "sleep"), l.primary.map { it.key })
        assertEquals(listOf("rm"), l.destructive.map { it.key })
    }

    @Test
    fun theAlbumMenuHasStarsAndGoesToTheArtist() {
        val l = menuLayout(songActions.filter { it.id != ActionIds.GO_TO_ALBUM && it.id != ActionIds.REMOVE_FROM_PLAYLIST && it.id != "somethingNew" }, MenuKind.Album, canGoToArtist = true, hasInfo = true)
        assertTrue(l.rating)
        assertEquals(listOf(ActionIds.ADD_TO_PLAYLIST, "download", "share", ActionIds.GO_TO_ARTIST), l.primary.map { it.key })
        assertEquals(listOf("pin", "info"), l.more.map { it.key })
    }

    @Test
    fun severalSongsKeepThePlayGroupStarsAndDownload() {
        val l = menuLayout(songActions.filter { it.id != "somethingNew" }, MenuKind.Songs)
        assertTrue(l.rating)
        assertEquals(listOf(ActionIds.ADD_TO_PLAYLIST, "download", "share"), l.primary.map { it.key })
        assertEquals(emptyList<String>(), l.more.map { it.key })
    }

    @Test
    fun shareOffersALinkForAnAlbumAndTheFileOnlyForOneSong() {
        assertTrue(shareChoices(MenuKind.Song).file && shareChoices(MenuKind.Song).songLink)
        assertTrue(!shareChoices(MenuKind.Songs).file && !shareChoices(MenuKind.Songs).songLink && shareChoices(MenuKind.Songs).navidrome)
        assertTrue(shareChoices(MenuKind.Album).songLink && !shareChoices(MenuKind.Album).file)
        assertTrue(shareChoices(MenuKind.Artist).songLink && !shareChoices(MenuKind.Artist).navidrome)
        assertTrue(shareChoices(MenuKind.Playlist).navidrome && !shareChoices(MenuKind.Playlist).songLink)
        assertTrue(!shareChoices(MenuKind.Other).any)
        assertEquals("https://song.link/s/Boards%20of%20Canada%20Roygbiv", songLinkSearch("Boards of Canada Roygbiv"))
        assertEquals("https://genius.com/search?q=%22Roygbiv%22%20%22Boards%20of%20Canada%22", geniusSearch("Roygbiv", "Boards of Canada"))
        assertNull(geniusSearch("  ", "Boards of Canada"))
    }

    @Test
    fun aMenuWithNothingInTheShortListPromotesMore() {
        val l = menuLayout(listOf(d(ActionIds.RESTORE_SAVED_QUEUE), d(ActionIds.DELETE_SAVED_QUEUE, destructive = true)), MenuKind.Other)
        assertEquals(listOf(ActionIds.RESTORE_SAVED_QUEUE), l.primary.map { it.key })
        assertEquals(listOf(ActionIds.DELETE_SAVED_QUEUE), l.destructive.map { it.key })
    }

    @Test
    fun groupsArePinnedRecentThenAllByNameAndOneMatchesGroupWhileSearching() {
        val lists = listOf(
            playlist("late", "Late night", changed = 1.0), playlist("drives", "Songs for long drives", changed = 2.0),
            playlist("run", "Run club", changed = 9.0), playlist("sunday", "Sunday slow", changed = 8.0),
            playlist("focus", "Focus", changed = 7.0), playlist("morning", "Morning coffee", changed = 3.0), playlist("road", "Road trip", changed = null),
            playlist("smart", "Smart", smart = true), playlist("theirs", "Theirs", mine = false),
        )
        val addable = addablePlaylists(lists)
        assertTrue(addable.none { it.id == "smart" || it.id == "theirs" })
        val sections = pickerSections(addable, listOf("late", "gone", "drives"), "")
        assertEquals(listOf(PickerGroup.Pinned, PickerGroup.Recent, PickerGroup.All), sections.map { it.group })
        assertEquals(listOf("late", "drives"), sections[0].playlists.map { it.id })
        assertEquals(listOf("run", "sunday", "focus"), sections[1].playlists.map { it.id })
        assertEquals(listOf("morning", "road"), sections[2].playlists.map { it.id })
        val matches = pickerSections(addable, listOf("late"), " DRIVE ")
        assertEquals(listOf(PickerGroup.Matches), matches.map { it.group })
        assertEquals(listOf("drives"), matches.single().playlists.map { it.id })
        assertEquals(emptyList<PickerSection>(), pickerSections(addable, emptyList(), "zzz"))
    }

    private fun membership(vararg entries: Pair<String, List<Pair<String, Int>>>) =
        entries.map { (id, refs) -> PlaylistMembership(id, refs.map { (t, i) -> PlaylistEntryRef(t, i.toUInt()) }, true) }

    @Test
    fun oneSongTicksBadgesAndDone() {
        val late = playlist("late", "Late night", 38)
        val sunday = playlist("sunday", "Sunday slow", 19)
        val run = playlist("run", "Run club", 52)
        var e = PickerEdits.of(listOf("t"), membership("late" to listOf("t" to 4), "sunday" to listOf("t" to 2, "t" to 7), "run" to emptyList()))
        assertEquals(PickerRow(Tick.On, null, PickerNote.None, 38), e.row(late))
        assertEquals(PickerRow(Tick.On, 2, PickerNote.None, 19), e.row(sunday))
        assertEquals(PickerRow(Tick.Off, null, PickerNote.None, 52), e.row(run))
        // Unticking a playlist holding it twice asks; "Remove one" drops the last copy.
        assertEquals(PickerTap.AskWhichCopies, e.tap("sunday"))
        e = e.removeOne("sunday")
        assertEquals(PickerRow(Tick.On, null, PickerNote.Removing, 18), e.row(sunday))
        e = (e.tap("run") as PickerTap.Changed).edits
        assertEquals(PickerRow(Tick.On, null, PickerNote.Adding, 53), e.row(run))
        e = e.addCopy("run")
        assertEquals(PickerRow(Tick.On, 2, PickerNote.Adding, 54), e.row(run))
        val ids = listOf("late", "sunday", "run")
        assertEquals(1 to 1, e.pending(ids))
        assertEquals(3, e.inCount(ids))
        assertEquals(listOf(Commands.playlistAdd("run", listOf("t", "t")), Commands.playlistRemove("sunday", listOf(7))), e.commands())
        assertEquals(PickerOutcome.AddedAndRemoved(1, 1), e.outcome(mapOf()))
        // Ticking again after unticking puts back what was there: no change.
        val untouched = PickerEdits.of(listOf("t"), membership("late" to listOf("t" to 4)))
        val back = ((untouched.tap("late") as PickerTap.Changed).edits.tap("late") as PickerTap.Changed).edits
        assertEquals(emptyList<Any>(), back.commands())
        assertNull(back.outcome(mapOf()))
    }

    @Test
    fun removingEveryCopyRemovesEveryIndexHighestFirst() {
        val e = PickerEdits.of(listOf("t"), membership("sunday" to listOf("t" to 2, "t" to 7, "t" to 5))).removeAll("sunday")
        assertEquals(listOf(Commands.playlistRemove("sunday", listOf(7, 5, 2))), e.commands())
        assertEquals(PickerOutcome.RemovedFrom("Sunday slow"), e.outcome(mapOf("sunday" to "Sunday slow")))
        assertEquals(PickerOutcome.RemovedSome(1, "Sunday slow"), PickerEdits.of(listOf("t"), membership("sunday" to listOf("t" to 2, "t" to 7))).removeOne("sunday").outcome(mapOf("sunday" to "Sunday slow")))
    }

    @Test
    fun outcomesNameTheOnePlaylistOrCountThem() {
        val base = PickerEdits.of(listOf("t"), membership("a" to emptyList(), "b" to emptyList(), "c" to listOf("t" to 0)))
        val names = mapOf("a" to "Run club", "c" to "Late night")
        assertEquals(PickerOutcome.AddedTo("Run club"), base.addCopy("a").outcome(names))
        assertEquals(PickerOutcome.AddedAgain("Late night"), base.addCopy("c").outcome(names))
        assertEquals(PickerOutcome.AddedToMany(2), base.addCopy("a").addCopy("b").outcome(names))
    }

    @Test
    fun severalSongsShowMixedAndTickingAddsTheMissingOnes() {
        val p = playlist("p", "Mix", 20)
        var e = PickerEdits.of(listOf("a", "b", "c"), membership("p" to listOf("a" to 3, "c" to 9)))
        assertEquals(Tick.Mixed, e.row(p).tick)
        e = (e.tap("p") as PickerTap.Changed).edits
        assertEquals(PickerRow(Tick.On, null, PickerNote.Adding, 21), e.row(p))
        assertEquals(listOf(Commands.playlistAdd("p", listOf("b"))), e.commands())
        // On -> off removes every copy of the songs; off -> back to what it held.
        e = (e.tap("p") as PickerTap.Changed).edits
        assertEquals(Tick.Off, e.row(p).tick)
        assertEquals(listOf(Commands.playlistRemove("p", listOf(9, 3))), e.commands())
        e = (e.tap("p") as PickerTap.Changed).edits
        assertEquals(Tick.Mixed, e.row(p).tick)
        assertEquals(emptyList<Any>(), e.commands())
        // Another copy of each (a and c: a second copy; b: its first), in one add.
        assertEquals(listOf(Commands.playlistAdd("p", listOf("a", "b", "c"))), e.addCopy("p").commands())
        // Two more of each: the copies go in rounds.
        assertEquals(listOf(Commands.playlistAdd("p", listOf("a", "b", "c", "a", "b", "c"))), e.addCopy("p").addCopy("p").commands())
    }
}
