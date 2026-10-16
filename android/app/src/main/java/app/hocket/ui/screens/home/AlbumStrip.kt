package app.hocket.ui.screens.home

import androidx.compose.foundation.clickable
import androidx.compose.foundation.indication
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ripple
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import app.hocket.R
import app.hocket.core.ArtworkSizes
import app.hocket.core.api.Album
import app.hocket.ui.components.Artwork
import app.hocket.ui.components.CarouselItemWidth
import app.hocket.ui.components.GridArtCorner
import app.hocket.ui.components.GridCellText

/**
 * A horizontal carousel of albums (Navic): 150 dp covers 12 dp apart inside a 16 dp margin, a
 * two-line title and the artist under each. The press ripple is drawn on the cover only.
 */
@Composable
fun AlbumStrip(albums: List<Album>, modifier: Modifier = Modifier, onClick: (Album) -> Unit) {
    LazyRow(modifier, contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        items(albums, key = { it.id }) { album ->
            val interaction = remember { MutableInteractionSource() }
            Column(Modifier.animateItem().width(CarouselItemWidth).clickable(interaction, indication = null) { onClick(album) }) {
                Artwork(album.coverArt, ArtworkSizes.GRID, stringResource(R.string.row_album_a11y, album.name, album.artist ?: ""),
                    Modifier.size(CarouselItemWidth).clip(RoundedCornerShape(GridArtCorner)).indication(interaction, ripple()), RoundedCornerShape(GridArtCorner))
                Spacer(Modifier.height(6.dp))
                GridCellText(album.name, album.artist ?: stringResource(R.string.unknown_artist))
            }
        }
    }
}
