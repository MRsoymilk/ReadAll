package xin.soymilk.readall

import java.nio.ByteBuffer

/** Narrow JNI contract. Keep native methods static on this exact JVM class. */
class NativeReader @JvmOverloads constructor(book: String, font: String, state: String, width: Int, height: Int, size: Int, margin: Int, pixelWidth: Int = width, pixelHeight: Int = height) : AutoCloseable {
    private var handle = nativeOpen(book, font, state, width, height, size, margin, pixelWidth, pixelHeight)
    init { check(handle > 0) { "ReadAll native reader creation failed" } }
    private fun requireOpen() { check(handle != 0L) { "ReadAll reader closed" } }
    @Synchronized fun state(): State { requireOpen(); return State(nativeState(handle)) }
    @Synchronized fun command(code: Int) = command(code, 0, 0)
    @Synchronized fun command(code: Int, a: Int, b: Int) { requireOpen(); nativeCommand(handle, code, a, b) }
    @Synchronized private fun checkedHandle(): Long { requireOpen(); return handle }

    /** Do not hold the input monitor while copying megabytes of immutable frame pixels. */
    fun copyPixels(state: State, target: ByteBuffer): Boolean {
        val owner = checkedHandle()
        require(target.isDirect && !target.isReadOnly && target.capacity() >= state.byteLength()) { "writable direct frame buffer required" }
        return nativeCopyPixels(owner, state.serial, target)
    }
    @Synchronized fun contents(): Array<Item> {
        requireOpen(); val fields = nativeContents(handle)
        check(fields != null && fields.size % 4 == 0) { "invalid native contents response" }
        return Array(fields.size / 4) { i -> Item(fields[i * 4], fields[i * 4 + 1].toInt(), fields[i * 4 + 2].toInt(), fields[i * 4 + 3].toInt()) }
    }
    @Synchronized fun viewport(width: Int, height: Int, pixelWidth: Int, pixelHeight: Int) { requireOpen(); nativeViewport(handle, width, height, pixelWidth, pixelHeight) }
    @Synchronized fun input(mode: String, text: String) { requireOpen(); nativeInput(handle, mode, text) }
    @Synchronized fun hostReply(kind: Int, text: String) { requireOpen(); nativeHostReply(handle, kind, text) }
    @Synchronized fun effects(): Array<String> { requireOpen(); val values = nativeEffects(handle); check(values != null && values.size % 2 == 0) { "invalid host effects" }; return values }
    @Synchronized override fun close() { if (handle != 0L) { val value = handle; handle = 0; nativeClose(value) } }

    class Item(@JvmField val title: String, @JvmField val depth: Int, @JvmField val spine: Int, @JvmField val offset: Int)
    class Preview(values: Array<String>?) {
        @JvmField val title: String; @JvmField val author: String; @JvmField val format: String
        @JvmField val width: Int; @JvmField val height: Int
        init {
            check(values != null && values.size == 5) { "invalid preview protocol" }
            title = values[0]; author = values[1]; format = values[2]; width = values[3].toInt(); height = values[4].toInt()
            check(width in 0..384 && height in 0..512 && (width == 0) == (height == 0)) { "invalid preview geometry" }
        }
    }
    /** The sole palette remains in Rust. Public fields also preserve existing JVM clients. */
    class Appearance(values: Array<String>?, offset: Int) {
        @JvmField val name: String
        @JvmField val canvas: Int; @JvmField val page: Int; @JvmField val panel: Int; @JvmField val ink: Int
        @JvmField val muted: Int; @JvmField val border: Int; @JvmField val accent: Int; @JvmField val onAccent: Int
        @JvmField val button: Int; @JvmField val selected: Int; @JvmField val hover: Int
        init {
            check(values != null && offset >= 0 && values.size == offset + 12 && values[offset] in arrayOf("light", "dark")) { "invalid theme protocol" }
            name = values[offset]
            val colors = IntArray(11) { i -> val value = values[offset + 1 + i].toLong(); check(value in 0xff000000L..0xffffffffL) { "invalid theme color" }; value.toInt() }
            canvas = colors[0]; page = colors[1]; panel = colors[2]; ink = colors[3]; muted = colors[4]; border = colors[5]
            accent = colors[6]; onAccent = colors[7]; button = colors[8]; selected = colors[9]; hover = colors[10]
        }
        fun dark() = name == "dark"
    }
    class State(values: Array<String>?) {
        @JvmField val status: String; @JvmField val phase: String; @JvmField val title: String; @JvmField val position: String
        @JvmField val percent: String; @JvmField val locator: String; @JvmField val notice: String; @JvmField val uiMode: String
        @JvmField val pageMode: String; @JvmField val input: String; @JvmField val animating: Boolean; @JvmField val editing: Boolean
        @JvmField val appearance: Appearance
        @JvmField val done: Long; @JvmField val total: Long; @JvmField val serial: Long; @JvmField val revision: Long
        @JvmField val width: Int; @JvmField val height: Int; @JvmField val logicalWidth: Int; @JvmField val logicalHeight: Int
        init {
            check(values != null && values.size == 33 && values[0] == "4") { "unsupported ReadAll native protocol" }
            status = values[1]; phase = values[2]; done = values[3].toLong(); total = values[4].toLong(); serial = values[5].toLong()
            width = values[6].toInt(); height = values[7].toInt(); title = values[8]; position = values[9]; percent = values[10]
            locator = values[11]; notice = values[12]; revision = values[13].toLong(); uiMode = values[14]; pageMode = values[15]
            animating = values[16] == "1"; editing = values[17] == "1"; input = values[18]
            logicalWidth = values[19].toInt(); logicalHeight = values[20].toInt(); appearance = Appearance(values, 21)
            check(width >= 0 && height >= 0 && logicalWidth >= 0 && logicalHeight >= 0 && width.toLong() * height <= 4194304L) { "invalid native frame geometry" }
        }
        fun busy() = status == "loading"
        fun closed() = status == "closed"
        fun byteLength(): Int = Math.toIntExact(width.toLong() * height * 4)
    }
    companion object {
        init { System.loadLibrary("readall_android") }
        const val NEXT = 1; const val PREVIOUS = 2; const val FIRST = 3; const val LAST = 4; const val LARGER = 5; const val SMALLER = 6
        const val CONTENTS = 7; const val JUMP = 8; const val THEME = 9; const val SAVE = 10; const val BOOKMARK = 11; const val RESIZE = 12
        const val BACK = 13; const val PAUSE = 14; const val FIND = 20; const val ANNOTATIONS = 21; const val SETTINGS = 22; const val SELECT = 23
        const val COPY = 24; const val PASTE = 25; const val NOTE = 26; const val HIGHLIGHT = 27; const val DELETE = 28; const val ACTIVATE = 29
        const val DISMISS = 30; const val BACKSPACE = 31; const val TOUCH = 40
        const val PREVIEW_BYTES = 384 * 512 * 4
        @JvmStatic fun preview(path: String, pixels: ByteBuffer): Preview {
            require(pixels.isDirect && !pixels.isReadOnly && pixels.capacity() >= PREVIEW_BYTES) { "preview needs a writable direct buffer" }
            return Preview(nativePreview(path, pixels))
        }
        @JvmStatic fun appearance(name: String) = Appearance(nativeAppearance("", name), 0)
        @JvmStatic fun loadAppearance(state: String) = Appearance(nativeAppearance(state, ""), 0)
        @JvmStatic fun saveTheme(state: String, name: String) = Appearance(nativeAppearance(state, name), 0)
        @JvmStatic private external fun nativeOpen(book: String, font: String, state: String, width: Int, height: Int, size: Int, margin: Int, pixelWidth: Int, pixelHeight: Int): Long
        @JvmStatic private external fun nativeViewport(handle: Long, width: Int, height: Int, pixelWidth: Int, pixelHeight: Int)
        @JvmStatic private external fun nativeState(handle: Long): Array<String>?
        @JvmStatic private external fun nativeContents(handle: Long): Array<String>?
        @JvmStatic private external fun nativeCommand(handle: Long, code: Int, a: Int, b: Int)
        @JvmStatic private external fun nativeCopyPixels(handle: Long, serial: Long, buffer: ByteBuffer): Boolean
        @JvmStatic private external fun nativeClose(handle: Long)
        @JvmStatic private external fun nativeInput(handle: Long, mode: String, text: String)
        @JvmStatic private external fun nativeHostReply(handle: Long, kind: Int, text: String)
        @JvmStatic private external fun nativeEffects(handle: Long): Array<String>?
        @JvmStatic private external fun nativeAppearance(state: String, requested: String): Array<String>?
        @JvmStatic private external fun nativePreview(path: String, pixels: ByteBuffer): Array<String>?
    }
}
