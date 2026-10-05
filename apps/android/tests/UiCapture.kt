package xin.soymilk.readall

import java.awt.image.BufferedImage
import java.io.File
import java.nio.ByteBuffer
import javax.imageio.ImageIO

/** Export actual shared reader pixels from generated fixtures, not screenshots of user books. */
object UiCapture {
    private fun save(reader: NativeReader, path: File) {
        repeat(100) {
            val s = reader.state()
            if (s.serial > 0 && !s.busy()) {
                val bytes = ByteBuffer.allocateDirect(s.byteLength())
                if (reader.copyPixels(s, bytes)) {
                    val image = BufferedImage(s.width, s.height, BufferedImage.TYPE_INT_ARGB)
                    for (y in 0 until s.height) for (x in 0 until s.width) { val r = bytes.get().toInt() and 255; val g = bytes.get().toInt() and 255; val b = bytes.get().toInt() and 255; val a = bytes.get().toInt() and 255; image.setRGB(x, y, (a shl 24) or (r shl 16) or (g shl 8) or b) }
                    check(ImageIO.write(image, "png", path)); println("$path ${s.uiMode} ${s.pageMode}"); return
                }
            }
            Thread.sleep(20)
        }
        throw AssertionError("capture timed out")
    }
    @JvmStatic fun main(args: Array<String>) {
        val root = File(args[0]); val font = File(args[1]); val width = args.getOrNull(2)?.toInt() ?: 400; val height = args.getOrNull(3)?.toInt() ?: 800
        val pw = args.getOrNull(4)?.toInt() ?: width; val ph = args.getOrNull(5)?.toInt() ?: height
        val output = File(root, "screens/${pw}x$ph"); output.mkdirs(); val started = System.nanoTime()
        NativeReader(File(root, "book.epub").path, font.path, File(root, "capture-state-$width-$height-$pw-$ph").path, width, height, 20, 16, pw, ph).use { reader ->
            JniSmoke.waitFor(reader) { it.serial > 0 && !it.busy() }; val revision = reader.state().revision; reader.command(NativeReader.PAUSE, 1, 0); JniSmoke.waitFor(reader) { it.revision > revision && !it.busy() }
            println("First frame ms=${(System.nanoTime() - started) / 1000000} logical=${width}x$height pixels=${pw}x$ph")
            save(reader, File(output, "$width-expanded.png")); reader.command(NativeReader.TOUCH + 1, width / 2, height - 176); JniSmoke.waitFor(reader) { it.uiMode == "collapsed" }; save(reader, File(output, "$width-collapsed.png"))
            reader.command(NativeReader.CONTENTS); JniSmoke.waitFor(reader) { it.uiMode == "toc" }; save(reader, File(output, "$width-toc.png"))
            reader.command(NativeReader.SETTINGS); JniSmoke.waitFor(reader) { it.uiMode == "settings" }; save(reader, File(output, "$width-settings.png"))
        }
    }
}
