package app.hocket.ui.components

import app.hocket.core.Commands
import app.hocket.core.api.Command
import app.hocket.core.api.Playlist
import app.hocket.core.api.PlaylistMembership

/** The add to playlist sheet's groups: pinned, recent, the rest; one "Matches" group while searching. */
enum class PickerGroup { Pinned, Recent, All, Matches }

/** One group of playlists as the sheet lists it. */
data class PickerSection(val group: PickerGroup, val playlists: List<Playlist>)

/** How many of the most recently changed playlists "Recent" shows. */
const val PICKER_RECENT = 3

/** The playlists songs can be added to: not smart (rules-driven), and the user's own. */
fun addablePlaylists(playlists: List<Playlist>): List<Playlist> = playlists.filter { !it.isSmart && it.isMine }

/**
 * The sheet's groups over [playlists] (already [addablePlaylists]): Pinned in the order of the
 * Library's playlist pins ([pinnedIds]), Recent (the [PICKER_RECENT] most recently changed of the
 * rest), All playlists (the others, by name). While [query] has text, one "Matches" group holds the
 * playlists whose name contains it, in the same order. Empty groups are left out.
 */
fun pickerSections(playlists: List<Playlist>, pinnedIds: List<String>, query: String): List<PickerSection> {
    val byId = playlists.associateBy { it.id }
    val pinned = pinnedIds.distinct().mapNotNull { byId[it] }
    val rest = playlists.filter { p -> pinned.none { it.id == p.id } }
    val recent = rest.filter { it.changed != null }.sortedByDescending { it.changed }.take(PICKER_RECENT)
    val all = rest.filter { p -> recent.none { it.id == p.id } }.sortedWith(compareBy({ it.name.lowercase() }, { it.id }))
    val needle = query.trim().lowercase()
    val sections = if (needle.isEmpty()) {
        listOf(PickerSection(PickerGroup.Pinned, pinned), PickerSection(PickerGroup.Recent, recent), PickerSection(PickerGroup.All, all))
    } else {
        listOf(PickerSection(PickerGroup.Matches, (pinned + recent + all).filter { it.name.lowercase().contains(needle) }))
    }
    return sections.filter { it.playlists.isNotEmpty() }
}

/** A row's checkbox: in the playlist, not, or (several songs) holding only some of them. */
enum class Tick { On, Off, Mixed }

/** A pending change a row notes after its song count. */
enum class PickerNote { None, Adding, Removing }

/** What a row shows: its tick, the copies badge (null: none), the note and its song count after the edit. */
data class PickerRow(val tick: Tick, val copies: Int?, val note: PickerNote, val songCount: Int)

/** What tapping a row does: change it, or ask which copies to remove first ("In Late night twice"). */
sealed interface PickerTap {
    data class Changed(val edits: PickerEdits) : PickerTap
    data object AskWhichCopies : PickerTap
}

/**
 * The sheet's state: for each playlist, the indices at which it holds each of [tracks] now
 * ([current], from `Query.PlaylistMembership`) and the copies the user wants ([target], only for
 * playlists they changed). Done turns the difference into commands ([commands]); everything here is
 * pure, so it is tested without a screen.
 */
data class PickerEdits(
    val tracks: List<String>,
    val current: Map<String, Map<String, List<Int>>>,
    val target: Map<String, Map<String, Int>> = emptyMap(),
) {
    /** Copies of each track [playlistId] holds now. */
    fun held(playlistId: String): Map<String, Int> = tracks.associateWith { current[playlistId]?.get(it)?.size ?: 0 }

    /** Copies of each track [playlistId] will hold after Done. */
    fun wanted(playlistId: String): Map<String, Int> = target[playlistId] ?: held(playlistId)

    fun changed(playlistId: String): Boolean = wanted(playlistId) != held(playlistId)

    private fun tickOf(counts: Map<String, Int>): Tick = when {
        counts.isNotEmpty() && counts.values.all { it > 0 } -> Tick.On
        counts.values.all { it == 0 } -> Tick.Off
        else -> Tick.Mixed
    }

    /** [playlist]'s row: its tick, the ×N badge when it holds every song more than once, the note and the count. */
    fun row(playlist: Playlist): PickerRow {
        val held = held(playlist.id)
        val wanted = wanted(playlist.id)
        val before = held.values.sum()
        val after = wanted.values.sum()
        val note = when {
            after > before -> PickerNote.Adding
            after < before -> PickerNote.Removing
            wanted != held -> PickerNote.Adding
            else -> PickerNote.None
        }
        val fewest = wanted.values.minOrNull() ?: 0
        return PickerRow(tickOf(wanted), fewest.takeIf { it > 1 }, note, (playlist.songCount.toInt() - before + after).coerceAtLeast(0))
    }

    private fun withCounts(playlistId: String, counts: Map<String, Int>): PickerEdits =
        copy(target = if (counts == held(playlistId)) target - playlistId else target + (playlistId to counts))

    /**
     * A tap on [playlistId]'s row. One song: tick adds it, untick removes it, or asks first when the
     * playlist holds it more than once. Several songs: a tick adds the ones it lacks, an untick
     * removes them all. Ticking a playlist again after unticking it puts back what it held.
     */
    fun tap(playlistId: String): PickerTap {
        val wanted = wanted(playlistId)
        val held = held(playlistId)
        return when (tickOf(wanted)) {
            Tick.Off -> PickerTap.Changed(withCounts(playlistId, if (tickOf(held) != Tick.Off) held else wanted.mapValues { (_, n) -> maxOf(n, 1) }))
            Tick.Mixed -> PickerTap.Changed(withCounts(playlistId, wanted.mapValues { (_, n) -> maxOf(n, 1) }))
            Tick.On -> if (tracks.size == 1 && wanted.values.single() > 1) PickerTap.AskWhichCopies else PickerTap.Changed(removeAll(playlistId))
        }
    }

    /** "Remove both" / "Remove all N": no copy of the songs left in [playlistId]. */
    fun removeAll(playlistId: String): PickerEdits = withCounts(playlistId, tracks.associateWith { 0 })

    /** "Remove one": one copy fewer of each song (the last one in the playlist goes). */
    fun removeOne(playlistId: String): PickerEdits = withCounts(playlistId, wanted(playlistId).mapValues { (_, n) -> (n - 1).coerceAtLeast(0) })

    /** "Add another copy" (or "Add" where none is in it yet): one more of each song. */
    fun addCopy(playlistId: String): PickerEdits = withCounts(playlistId, wanted(playlistId).mapValues { (_, n) -> n + 1 })

    /** How many copies the dialog's title counts ("twice"): the one song's pending copies. */
    fun copies(playlistId: String): Int = wanted(playlistId).values.maxOrNull() ?: 0

    /** Playlists that will hold any of the songs after Done. */
    fun inCount(playlistIds: Collection<String>): Int = playlistIds.count { id -> wanted(id).values.any { it > 0 } }

    /** Playlists gaining and losing copies, for the summary line ("1 adding · 1 removing"). */
    fun pending(playlistIds: Collection<String>): Pair<Int, Int> {
        var adding = 0
        var removing = 0
        playlistIds.filter(::changed).forEach { id ->
            val before = held(id).values.sum()
            val after = wanted(id).values.sum()
            if (after >= before) adding++ else removing++
        }
        return adding to removing
    }

    /**
     * Done, as the core's commands: per changed playlist, one `PlaylistRemove` of every index going
     * (highest first: the last copies of a song go first, and the indices stay valid as they are
     * removed), then one `PlaylistAdd` with an entry per added copy.
     */
    fun commands(): List<Command> = target.keys.sorted().filter(::changed).flatMap { id ->
        val held = held(id)
        val wanted = wanted(id)
        val removed = tracks.flatMap { t ->
            val drop = (held.getValue(t) - wanted.getValue(t)).coerceAtLeast(0)
            current[id]?.get(t).orEmpty().sorted().takeLast(drop)
        }.sortedDescending()
        val rounds = tracks.maxOfOrNull { t -> wanted.getValue(t) - held.getValue(t) } ?: 0
        val added = (1..rounds).flatMap { round -> tracks.filter { t -> wanted.getValue(t) - held.getValue(t) >= round } }
        listOfNotNull(
            removed.takeIf { it.isNotEmpty() }?.let { Commands.playlistRemove(id, it) },
            added.takeIf { it.isNotEmpty() }?.let { Commands.playlistAdd(id, it) },
        )
    }

    /** What Done did, for its snackbar (null when nothing changed). */
    fun outcome(names: Map<String, String>): PickerOutcome? {
        val changes = target.keys.filter(::changed)
        if (changes.isEmpty()) return null
        if (changes.size == 1) {
            val id = changes.single()
            val name = names[id] ?: ""
            val before = held(id).values.sum()
            val after = wanted(id).values.sum()
            return when {
                after >= before -> if (before > 0) PickerOutcome.AddedAgain(name) else PickerOutcome.AddedTo(name)
                after == 0 -> PickerOutcome.RemovedFrom(name)
                else -> PickerOutcome.RemovedSome(before - after, name)
            }
        }
        val (adding, removing) = pending(changes)
        return when {
            removing == 0 -> PickerOutcome.AddedToMany(adding)
            adding == 0 -> PickerOutcome.RemovedFromMany(removing)
            else -> PickerOutcome.AddedAndRemoved(adding, removing)
        }
    }

    companion object {
        /** The sheet's starting state from the core's membership answer. */
        fun of(tracks: List<String>, membership: List<PlaylistMembership>): PickerEdits = PickerEdits(
            tracks.distinct(),
            membership.associate { m -> m.playlistId to m.entries.groupBy({ it.trackId }, { it.index.toInt() }) },
        )
    }
}

/** The snackbar after Done ("Added to Run club", "Removed one from Sunday slow", "Added to 2 playlists"). */
sealed interface PickerOutcome {
    data class AddedTo(val name: String) : PickerOutcome
    data class AddedAgain(val name: String) : PickerOutcome
    data class RemovedFrom(val name: String) : PickerOutcome
    data class RemovedSome(val count: Int, val name: String) : PickerOutcome
    data class AddedToMany(val playlists: Int) : PickerOutcome
    data class RemovedFromMany(val playlists: Int) : PickerOutcome
    data class AddedAndRemoved(val added: Int, val removed: Int) : PickerOutcome
}
