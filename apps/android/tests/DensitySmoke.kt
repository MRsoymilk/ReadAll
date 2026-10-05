package xin.soymilk.readall

import java.io.File
import java.nio.ByteBuffer

object DensitySmoke {
    @JvmStatic fun main(args: Array<String>) {
        val root = File(args[0]); val book = File(root, "book.epub").path; val font = File(root, "font.ttf").path
        NativeReader(book, font, File(root, "low-density-state").path, 400, 520, 16, 16).use { low ->
            NativeReader(book, font, File(root, "high-density-state").path, 400, 520, 16, 16, 1080, 1404).use { high ->
                val a = JniSmoke.waitFor(low) { it.serial > 0 && !it.busy() }; val b = JniSmoke.waitFor(high) { it.serial > 0 && !it.busy() }
                check(a.locator == b.locator && a.position == b.position); check(b.width == 1080 && b.height == 1404 && b.logicalWidth == 400 && b.logicalHeight == 520)
                val pixels = ByteBuffer.allocateDirect(b.byteLength()); check(high.copyPixels(b, pixels)); check(pixels.capacity() == 1080 * 1404 * 4)
                high.command(NativeReader.TOUCH, 200, 344); high.command(NativeReader.TOUCH + 1, 200, 344); JniSmoke.waitFor(high) { it.uiMode == "collapsed" }
                high.viewport(400, 520, 800, 1040); val resized = JniSmoke.waitFor(high) { it.width == 800 && !it.busy() }; check(resized.locator == b.locator && resized.logicalWidth == 400)
            }
        }
        println("PASS Kotlin/JNI high-density pixels, same pagination, touch targets and density-only resize")
    }
}
