package xin.soymilk.readall

import java.io.File
import java.nio.ByteBuffer
import java.security.MessageDigest

/** Actual JVM/JNI tests with isolated files; no Android stubs. */
object JniSmoke {
    @JvmStatic fun require(value: Boolean, message: String) { if (!value) throw AssertionError(message) }
    fun waitFor(reader: NativeReader, done: (NativeReader.State) -> Boolean): NativeReader.State {
        val deadline = System.nanoTime() + 30000000000L
        while (true) { val state = reader.state(); if (done(state)) return state; require(!state.closed(), "native reader stopped: ${state.notice}"); require(System.nanoTime() < deadline, "native reader timeout: ${state.phase} ${state.notice}"); Thread.sleep(10) }
    }
    @JvmStatic fun main(args: Array<String>) {
        val root = File(args[0]).canonicalFile; val book = File(root, "book.epub"); val font = File(root, "font.ttf"); val state = File(root, "state")
        val before = hash(book); var anchor = ""
        NativeReader(book.path, font.path, state.path, 400, 520, 16, 24).use { reader ->
            val first = waitFor(reader) { it.serial > 0 && !it.busy() }; val pixels = ByteBuffer.allocateDirect(first.byteLength())
            require(reader.copyPixels(first, pixels), "initial frame copy failed")
            var colors = 0
            for (i in 0 until pixels.capacity() step 4) if ((pixels[i].toInt() and 255) == 10 && (pixels[i + 1].toInt() and 255) == 90 && (pixels[i + 2].toInt() and 255) == 180 && (pixels[i + 3].toInt() and 255) == 255) colors++
            require(colors > 0, "real image pixels did not cross JNI in RGBA order"); require(first.uiMode == "expanded", "shared toolbar not enabled")
            reader.command(NativeReader.TOUCH, 200, 344); reader.command(NativeReader.TOUCH + 1, 200, 344); waitFor(reader) { it.uiMode == "collapsed" }
            reader.command(NativeReader.TOUCH, 200, 493); reader.command(NativeReader.TOUCH + 1, 200, 493); waitFor(reader) { it.uiMode == "expanded" }
            expect<IllegalArgumentException>("heap/small buffer accepted") { reader.copyPixels(first, ByteBuffer.allocate(4)) }
            expect<IllegalStateException>("unknown native opcode accepted") { reader.command(999) }
            reader.command(NativeReader.NEXT); val second = waitFor(reader) { it.locator != first.locator && !it.busy() }; require(second.locator != first.locator, "next did not navigate")
            pixels.put(0, 77); require(!reader.copyPixels(first, pixels), "stale frame must not be copied"); require(pixels[0] == 77.toByte(), "stale frame changed destination")
            reader.command(NativeReader.CONTENTS); waitFor(reader) { !it.busy() && reader.contents().size == 2 }
            val contents = reader.contents(); require(contents[1].spine == 1, "wrong chapter mapping"); reader.command(NativeReader.JUMP, contents[1].spine, contents[1].offset)
            anchor = waitFor(reader) { it.position.contains("第 2/2 章") && !it.busy() }.locator
            reader.command(NativeReader.RESIZE, 500, 600); val resized = waitFor(reader) { it.width == 500 && !it.busy() }; require(anchor == resized.locator, "resize lost anchor")
            reader.command(NativeReader.THEME); waitFor(reader) { it.serial > resized.serial && !it.busy() }; reader.command(NativeReader.BOOKMARK); waitFor(reader) { it.notice.contains("书签已保存") }
            reader.command(NativeReader.SETTINGS); waitFor(reader) { it.uiMode == "settings" }; repeat(4) { reader.command(NativeReader.NEXT) }
            for (mode in arrayOf("book", "scroll", "slide")) { reader.command(NativeReader.LARGER); waitFor(reader) { it.pageMode == mode } }
            reader.command(NativeReader.BACK); waitFor(reader) { it.uiMode == "expanded" }
            reader.command(NativeReader.FIND); waitFor(reader) { it.editing }; reader.input("search", "中文😀"); waitFor(reader) { it.input == "中文😀" }
            reader.command(NativeReader.PASTE); var effects = emptyArray<String>(); val deadline = System.nanoTime() + 5000000000L
            while (effects.isEmpty() && System.nanoTime() < deadline) { effects = reader.effects(); Thread.sleep(5) }
            require(effects.size == 2 && effects[0] == "paste", "clipboard request not routed"); require(reader.effects().isEmpty(), "host request replayed")
            reader.hostReply(2, " World"); waitFor(reader) { it.input == "中文😀 World" }; reader.command(NativeReader.BACK); waitFor(reader) { !it.editing }
            reader.command(NativeReader.PAUSE, 1, 0); reader.command(NativeReader.PAUSE, 0, 0)
        }
        Thread.sleep(150)
        NativeReader(book.path, font.path, state.path, 500, 600, 16, 24).use { resumed ->
            require(anchor == waitFor(resumed) { it.serial > 0 && !it.busy() }.locator, "saved progress did not restore")
            resumed.close(); resumed.close(); expect<IllegalStateException>("closed handle accessible") { resumed.state() }
        }
        require(before.contentEquals(hash(book)), "input book changed")
        println("PASS Kotlin/JVM JNI: publication, RGBA, buffers, errors, navigation, toolbar/TOC/settings, modes, UTF-8, clipboard, resize, theme, bookmark, restart, close and immutable source")
    }
    fun hash(file: File): ByteArray = MessageDigest.getInstance("SHA-256").digest(file.readBytes())
    inline fun <reified T : Throwable> expect(message: String, block: () -> Unit) { try { block() } catch (error: Throwable) { if (error is T) return; throw error }; throw AssertionError(message) }
}
