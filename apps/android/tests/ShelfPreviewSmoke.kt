package xin.soymilk.readall

import java.awt.image.BufferedImage
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.FileOutputStream
import java.lang.reflect.InvocationTargetException
import java.nio.ByteBuffer
import java.util.zip.CRC32
import java.util.zip.ZipEntry
import java.util.zip.ZipOutputStream
import javax.imageio.ImageIO

object ShelfPreviewSmoke {
    private fun add(zip: ZipOutputStream, name: String, data: ByteArray, stored: Boolean = false) {
        val entry = ZipEntry(name)
        if (stored) { val crc = CRC32().apply { update(data) }; entry.method = ZipEntry.STORED; entry.size = data.size.toLong(); entry.compressedSize = data.size.toLong(); entry.crc = crc.value }
        zip.putNextEntry(entry); zip.write(data); zip.closeEntry()
    }
    private fun fixture(root: File, name: String, epub3: Boolean, image: ByteArray): File {
        val book = File(root, name)
        ZipOutputStream(FileOutputStream(book)).use { zip ->
            add(zip, "mimetype", "application/epub+zip".toByteArray(), true)
            add(zip, "META-INF/container.xml", "<container><rootfiles><rootfile full-path='OPS/book.opf' media-type='application/oebps-package+xml'/></rootfiles></container>".toByteArray())
            val meta = if (epub3) "" else "<meta name='cover' content='front'/>"; val property = if (epub3) " properties='cover-image'" else ""
            add(zip, "OPS/book.opf", ("<package xmlns:dc='http://purl.org/dc/elements/1.1/'><metadata><dc:title>书库预览😀</dc:title><dc:creator>测试作者</dc:creator>$meta</metadata><manifest><item id='chapter' href='chapter.xhtml' media-type='application/xhtml+xml'/><item id='front' href='front.png' media-type='image/png'$property/></manifest><spine><itemref idref='chapter'/></spine></package>").toByteArray())
            add(zip, "OPS/chapter.xhtml", "<html><body>正文没有参与封面排版。</body></html>".toByteArray()); add(zip, "OPS/front.png", image)
        }
        return book
    }
    @JvmStatic fun main(args: Array<String>) {
        val root = File(args[0]); val image = BufferedImage(800, 1000, BufferedImage.TYPE_INT_RGB); val row = IntArray(800) { 0x0a5ab4 }
        repeat(1000) { image.setRGB(0, it, 800, 1, row, 0, 800) }; val out = ByteArrayOutputStream(); ImageIO.write(image, "png", out)
        val pixels = ByteBuffer.allocateDirect(NativeReader.PREVIEW_BYTES)
        for (epub3 in arrayOf(false, true)) {
            val book = fixture(root, "shelf-preview-$epub3.epub", epub3, out.toByteArray()); val before = JniSmoke.hash(book); val p = NativeReader.preview(book.absolutePath, pixels)
            check(p.title == "书库预览😀" && p.author == "测试作者" && p.format == "EPUB"); check(p.width == 384 && p.height == 480)
            check((pixels[0].toInt() and 255) == 10 && (pixels[1].toInt() and 255) == 90 && (pixels[2].toInt() and 255) == 180 && (pixels[3].toInt() and 255) == 255)
            check(before.contentEquals(JniSmoke.hash(book)))
        }
        val bad = fixture(root, "shelf-preview-no-image.epub", false, "broken image".toByteArray()); check(NativeReader.preview(bad.absolutePath, pixels).width == 0)
        JniSmoke.expect<IllegalArgumentException>("read-only buffer accepted") { NativeReader.preview(bad.absolutePath, pixels.asReadOnlyBuffer()) }
        val call = NativeReader::class.java.getDeclaredMethod("nativePreview", String::class.java, ByteBuffer::class.java).apply { isAccessible = true }
        var rejected = false; try { call.invoke(null, bad.absolutePath, ByteBuffer.allocateDirect(4)) } catch (e: InvocationTargetException) { rejected = e.cause is IllegalStateException }; check(rejected)
        pixels.put(0, 77); JniSmoke.expect<IllegalStateException>("relative path accepted") { NativeReader.preview("relative.epub", pixels) }; check(pixels[0] == 77.toByte())
        println("PASS Kotlin/JNI EPUB2/3 previews: metadata/Unicode, bounded covers, RGBA, broken images, buffers, paths and unchanged source")
    }
}
