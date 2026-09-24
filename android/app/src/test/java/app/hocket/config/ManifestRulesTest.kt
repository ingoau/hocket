package app.hocket.config

import java.io.File
import javax.xml.parsers.DocumentBuilderFactory
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.w3c.dom.Element

/**
 * The shipped backup rules and manifests, read as XML: what must never leave the device (or arrive
 * on another one) and which components are exported.
 */
class ManifestRulesTest {
    private fun file(path: String): File {
        var dir: File? = File(System.getProperty("user.dir")).absoluteFile
        while (dir != null) {
            File(dir, path).takeIf { it.exists() }?.let { return it }
            dir = dir.parentFile
        }
        error("$path not found")
    }

    private fun parse(path: String) = DocumentBuilderFactory.newInstance().apply { isNamespaceAware = true }.newDocumentBuilder().parse(file(path))

    private fun Element.children(tag: String): List<Element> =
        (0 until childNodes.length).map { childNodes.item(it) }.filterIsInstance<Element>().filter { it.tagName == tag }

    /** Everything the device must keep to itself: the library mirror and its backups (large, and a
     * cache), the Connect device id (must be unique per install) and the keystore ciphertext and names. */
    private val mustExclude = setOf(
        "file:hocket/hocket.sqlite", "file:hocket/hocket.sqlite-wal", "file:hocket/hocket.sqlite-shm", "file:hocket/backups",
        "file:hocket/downloads", "file:hocket/cache",
        "sharedpref:hocket-core.xml", "sharedpref:hocket-credentials.xml", "sharedpref:hocket-credential-names.xml",
    )

    private fun excludes(section: Element): Set<String> = section.children("exclude").map { it.getAttribute("domain") + ":" + it.getAttribute("path") }.toSet()

    @Test
    fun autoBackupNeverCarriesTheMirrorTheDeviceIdOrTheCredentials() {
        val root = parse("android/app/src/main/res/xml/backup_rules.xml").documentElement
        assertEquals("full-backup-content", root.tagName)
        val missing = mustExclude - excludes(root)
        assertTrue("backup_rules.xml must exclude $missing", missing.isEmpty())
    }

    @Test
    fun cloudBackupAndDeviceTransferExcludeTheSame() {
        val root = parse("android/app/src/main/res/xml/data_extraction_rules.xml").documentElement
        for (tag in listOf("cloud-backup", "device-transfer")) {
            val section = root.children(tag).single()
            val missing = mustExclude - excludes(section)
            assertTrue("$tag must exclude $missing", missing.isEmpty())
        }
    }

    @Test
    fun noLegacyMediaButtonReceiverIsDeclared() {
        val manifest = file("android/playback/src/main/AndroidManifest.xml").readText()
        // Media3 declares its own receiver; the exported legacy one let any app start playback.
        assertFalse(manifest.contains("androidx.media.session.MediaButtonReceiver"))
        assertFalse(Regex("<receiver\\b").containsMatchIn(manifest))
    }
}
