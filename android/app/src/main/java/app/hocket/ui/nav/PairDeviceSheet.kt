package app.hocket.ui.nav

import android.content.Intent
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.hocket.R
import app.hocket.ui.LocalCoreClient
import app.hocket.ui.icons.HocketIcons
import com.google.zxing.BarcodeFormat
import com.google.zxing.EncodeHintType
import com.google.zxing.common.BitMatrix
import com.google.zxing.qrcode.QRCodeWriter
import com.google.zxing.qrcode.decoder.ErrorCorrectionLevel


/**
 * Add a device, from the account sheet: the core makes a pairing invite (this identity, this
 * device's key, a one-time secret, where to reach it, account configs without passwords) and this
 * shows it as a QR code, which Hocket's setup scanner or a phone camera opens on the other device;
 * Share link sends the same link (to a computer, say). It works once, for 10 minutes.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PairDeviceSheet(onDismiss: () -> Unit) {
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true), modifier = Modifier.testTag("pairDevice.sheet")) {
        val client = LocalCoreClient.current
        LaunchedEffect(client) { client.createPairingInvite() }
        val invite by client.pairingInvite.collectAsStateWithLifecycle()
        PairDeviceSheetContent(invite?.link, invite?.expires_at)
    }
}

/** The sheet's content for the invite [link] (also drawn on its own by tests). */
@Composable
fun PairDeviceSheetContent(link: String?, expiresAt: Double?) {
    val context = LocalContext.current
    Column(Modifier.verticalScroll(rememberScrollState()).navigationBarsPadding().padding(start = 24.dp, end = 24.dp, bottom = 24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        Text(stringResource(R.string.pair_device_title), style = MaterialTheme.typography.titleLarge, modifier = Modifier.fillMaxWidth().semantics { heading() })
        Text(stringResource(R.string.pair_device_body), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.fillMaxWidth().padding(top = 4.dp, bottom = 20.dp))
        // Dark modules on white in either theme: scanners expect a light quiet zone around a dark code.
        val codeDescription = stringResource(R.string.pair_device_code)
        Box(
            Modifier.widthIn(max = 280.dp).fillMaxWidth().aspectRatio(1f).clip(RoundedCornerShape(24.dp)).background(Color.White).padding(20.dp)
                .semantics { contentDescription = codeDescription }.testTag("pairDevice.code"),
        ) {
            if (link != null) QrCode(link, Modifier.fillMaxWidth().aspectRatio(1f))
        }
        if (expiresAt != null) {
            val until = remember(expiresAt) { java.text.DateFormat.getTimeInstance(java.text.DateFormat.SHORT).format(java.util.Date(expiresAt.toLong())) }
            Text(stringResource(R.string.pair_device_waiting, until), style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(top = 12.dp).testTag("pairDevice.status"))
        }
        OutlinedButton(
            onClick = {
                val send = Intent(Intent.ACTION_SEND).apply { type = "text/plain"; putExtra(Intent.EXTRA_TEXT, link) }
                context.startActivity(Intent.createChooser(send, null))
            },
            enabled = link != null,
            shapes = ButtonDefaults.shapes(), modifier = Modifier.padding(top = 8.dp).testTag("pairDevice.share"),
        ) {
            Icon(HocketIcons.Filled.Share, null, Modifier.size(ButtonDefaults.IconSize))
            Text(stringResource(R.string.pair_device_share), Modifier.padding(start = ButtonDefaults.IconSpacing))
        }
    }
}

/** [text] as a QR code, black modules filling the box (the caller draws the quiet zone). */
@Composable
internal fun QrCode(text: String, modifier: Modifier) {
    val matrix = remember(text) { qrMatrix(text) }
    Canvas(modifier) {
        val cell = size.width / matrix.width
        for (y in 0 until matrix.height) for (x in 0 until matrix.width) {
            // A hair wider than a module, so neighbours meet without anti-aliased seams.
            if (matrix[x, y]) drawRect(Color.Black, Offset(x * cell, y * cell), Size(cell + 0.5f, cell + 0.5f))
        }
    }
}

/** The modules of [text]'s QR code, one cell per module and no margin. */
internal fun qrMatrix(text: String): BitMatrix =
    QRCodeWriter().encode(text, BarcodeFormat.QR_CODE, 0, 0, mapOf(EncodeHintType.ERROR_CORRECTION to ErrorCorrectionLevel.M, EncodeHintType.MARGIN to 0, EncodeHintType.CHARACTER_SET to "UTF-8"))
