package xin.soymilk.readall

import java.io.File
import java.lang.reflect.Modifier
import java.nio.ByteBuffer
import java.util.concurrent.CompletableFuture
import java.util.concurrent.TimeUnit

object SmoothScrollSmoke {
    private fun pixels(r: NativeReader): ByteBuffer {
        repeat(100) { val s = r.state(); val buffer = ByteBuffer.allocateDirect(s.byteLength()); if (s.serial > 0 && !s.busy() && r.copyPixels(s, buffer)) return buffer; Thread.sleep(10) }
        throw AssertionError("cannot capture stable frame")
    }
    @JvmStatic fun main(args: Array<String>) {
        check(!Modifier.isSynchronized(NativeReader::class.java.getMethod("copyPixels", NativeReader.State::class.java, ByteBuffer::class.java).modifiers))
        val root = File(args[0])
        NativeReader(File(root, "toc.epub").path, File(root, "font.ttf").path, File(root, "smooth-state").path, 400, 640, 16, 24, 1080, 1728).use { r ->
            val first = JniSmoke.waitFor(r) { it.serial > 0 && !it.busy() }; r.command(NativeReader.CONTENTS); JniSmoke.waitFor(r) { it.uiMode == "toc" && !it.busy() }
            r.command(NativeReader.PAUSE, 1, 0); Thread.sleep(40); val still = r.state(); val before = pixels(r)
            val touch = TouchRouter(8f) { kind, x, y -> r.command(NativeReader.TOUCH + kind, x, y) }
            touch.down(200, 260); touch.move(200, 250); touch.move(200, 248); touch.up(200, 248, 0)
            JniSmoke.waitFor(r) { it.serial > still.serial && !it.busy() }; Thread.sleep(40); val after = pixels(r)
            var changes = 0; for (i in 0 until after.capacity()) if (after[i] != before[i]) changes++
            check(changes > 100) { "sub-row drag still snaps" }; check(r.state().locator == first.locator)
            val state = r.state(); val buffer = ByteBuffer.allocateDirect(state.byteLength())
            val copying = CompletableFuture.runAsync { repeat(20) { r.copyPixels(state, buffer) } }
            repeat(20) { r.state(); r.command(NativeReader.SAVE) }; copying.get(15, TimeUnit.SECONDS)
            r.command(NativeReader.PAUSE, 0, 0); touch.down(200, 260); touch.move(200, 248); touch.up(200, 248, 150)
            JniSmoke.waitFor(r) { it.animating && it.uiMode == "toc" }; touch.down(200, 248)
            JniSmoke.waitFor(r) { !it.animating && it.uiMode == "toc" && !it.busy() }; touch.cancel(); check(r.state().locator == first.locator)
        }
        println("PASS Kotlin/JNI sub-row pixels, inertia/stop, high-density frames and concurrent copy/input")
    }
}
