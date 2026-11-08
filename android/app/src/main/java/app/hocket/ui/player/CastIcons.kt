package app.hocket.ui.player

import androidx.compose.ui.graphics.vector.ImageVector
import app.hocket.playback.CastDevices.Kind
import app.hocket.ui.icons.HocketIcons

/** A Cast receiver's icon by what it is: filled for the connect button, outlined for the Connect sheet's rows. */
internal fun castIcon(kind: Kind?, filled: Boolean): ImageVector = if (filled) when (kind) {
    Kind.Group -> HocketIcons.Filled.SpeakerGroup
    Kind.NestMini -> HocketIcons.Filled.NestMini
    Kind.HomeMini -> HocketIcons.Filled.HomeMini
    Kind.NestAudio -> HocketIcons.Filled.NestAudio
    Kind.HomeMax -> HocketIcons.Filled.HomeMax
    Kind.NestHub -> HocketIcons.Filled.NestDisplay
    Kind.NestHubMax -> HocketIcons.Filled.NestDisplayMax
    Kind.Chromecast -> HocketIcons.Filled.Chromecast2
    Kind.ChromecastGoogleTv -> HocketIcons.Filled.ChromecastDevice
    Kind.WifiPoint -> HocketIcons.Filled.NestWifiPoint
    Kind.Tv -> HocketIcons.Filled.Tv
    Kind.Speaker -> HocketIcons.Filled.Speaker
    Kind.Unknown, null -> HocketIcons.Filled.Cast
} else when (kind) {
    Kind.Group -> HocketIcons.Outlined.SpeakerGroup
    Kind.NestMini -> HocketIcons.Outlined.NestMini
    Kind.HomeMini -> HocketIcons.Outlined.HomeMini
    Kind.NestAudio -> HocketIcons.Outlined.NestAudio
    Kind.HomeMax -> HocketIcons.Outlined.HomeMax
    Kind.NestHub -> HocketIcons.Outlined.NestDisplay
    Kind.NestHubMax -> HocketIcons.Outlined.NestDisplayMax
    Kind.Chromecast -> HocketIcons.Outlined.Chromecast2
    Kind.ChromecastGoogleTv -> HocketIcons.Outlined.ChromecastDevice
    Kind.WifiPoint -> HocketIcons.Outlined.NestWifiPoint
    Kind.Tv -> HocketIcons.Outlined.Tv
    Kind.Speaker -> HocketIcons.Outlined.Speaker
    Kind.Unknown, null -> HocketIcons.Outlined.Cast
}
