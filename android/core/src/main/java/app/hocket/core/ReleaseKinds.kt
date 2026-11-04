package app.hocket.core

import app.hocket.core.api.Album
import app.hocket.core.api.ReleaseKind

/**
 * An album's release kind for the discography shelves, as the core decides it
 * (`Album::release_kind`): a compilation first, then an EP, then a single, anything else an album.
 * Release types come from OpenSubsonic `releaseTypes`, lowercased by the core.
 */
fun Album.releaseKind(): ReleaseKind {
    val types = releaseTypes.orEmpty()
    return when {
        isCompilation || "compilation" in types -> ReleaseKind.Compilation
        "ep" in types -> ReleaseKind.Ep
        "single" in types -> ReleaseKind.Single
        else -> ReleaseKind.Album
    }
}

/** The albums grouped by kind in shelf order (albums, EPs, singles, compilations), each keeping its order; empty kinds left out. */
fun List<Album>.byReleaseKind(): List<Pair<ReleaseKind, List<Album>>> =
    ReleaseKind.entries.map { kind -> kind to filter { it.releaseKind() == kind } }.filter { it.second.isNotEmpty() }
