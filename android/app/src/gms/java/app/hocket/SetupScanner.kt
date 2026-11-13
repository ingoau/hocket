package app.hocket

import android.content.Context
import com.google.mlkit.vision.barcode.common.Barcode
import com.google.mlkit.vision.codescanner.GmsBarcodeScannerOptions
import com.google.mlkit.vision.codescanner.GmsBarcodeScanning

/**
 * Scanning another device's setup code with Google's code scanner: Play services draws the
 * camera UI, so the app needs no camera permission. [onResult] gets the code's text, [onError] runs
 * when the scanner could not start (no camera, its module not downloaded yet); cancelling calls neither.
 */
internal object SetupScanner {
    const val available = true

    fun scan(context: Context, onResult: (String) -> Unit, onError: () -> Unit) {
        val options = GmsBarcodeScannerOptions.Builder().setBarcodeFormats(Barcode.FORMAT_QR_CODE).build()
        GmsBarcodeScanning.getClient(context, options).startScan()
            .addOnSuccessListener { code -> code.rawValue?.let(onResult) }
            .addOnFailureListener { onError() }
    }
}
