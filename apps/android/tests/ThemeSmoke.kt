package xin.soymilk.readall

import java.io.File
import java.nio.ByteBuffer

object ThemeSmoke {
    @JvmStatic fun color(b: ByteBuffer, x: Int, y: Int, width: Int): Int { val i = (y * width + x) * 4; return ((b[i + 3].toInt() and 255) shl 24) or ((b[i].toInt() and 255) shl 16) or ((b[i + 1].toInt() and 255) shl 8) or (b[i + 2].toInt() and 255) }
    @JvmStatic fun main(args: Array<String>) {
        val root = File(args[0]); val state = File(root, "theme-smoke-state"); val light = NativeReader.appearance("light"); val dark = NativeReader.appearance("dark")
        check(!light.dark() && dark.dark() && light.panel != dark.panel)
        check(NativeReader.appearance("paper").name == "light" && NativeReader.appearance("sepia").name == "light")
        JniSmoke.expect<IllegalStateException>("unknown theme accepted") { NativeReader.appearance("invalid") }
        NativeReader.saveTheme(state.path, "dark"); check(NativeReader.loadAppearance(state.path).dark())
        val settings = File(state, "library-v1/settings.conf"); val before = settings.readText(); check("size=20\n" in before && "margin=16\n" in before)
        var anchor = ""
        NativeReader(File(root, "book.epub").path, File(root, "font.ttf").path, state.path, 400, 640, 20, 16, 1080, 1728).use { r ->
            var s = JniSmoke.waitFor(r) { it.serial > 0 && !it.busy() }; anchor = s.locator; check(s.appearance.dark())
            for (name in arrayOf("light", "dark")) {
                r.command(NativeReader.THEME); s = JniSmoke.waitFor(r) { it.appearance.name == name && !it.busy() }; check(s.locator == anchor)
                val revision = s.revision; r.command(NativeReader.PAUSE, 1, 0); s = JniSmoke.waitFor(r) { it.revision > revision && !it.busy() }
                val bytes = ByteBuffer.allocateDirect(s.byteLength()); check(r.copyPixels(s, bytes)); check(color(bytes, 0, 0, s.width) == s.appearance.page)
                check(color(bytes, s.width / 2, 1220, s.width) == s.appearance.panel); check(color(bytes, 38, 1220, s.width) != s.appearance.panel)
                r.command(NativeReader.SETTINGS); JniSmoke.waitFor(r) { it.uiMode == "settings" && !it.busy() }; r.command(NativeReader.BACK); JniSmoke.waitFor(r) { it.uiMode == "expanded" && !it.busy() }
            }
        }
        check(NativeReader.loadAppearance(state.path).dark()); NativeReader.saveTheme(state.path, "light")
        check(before.replace("theme=dark", "theme=light") == settings.readText())
        NativeReader(File(root, "book.epub").path, File(root, "font.ttf").path, state.path, 400, 640, 20, 16).use { r -> val s = JniSmoke.waitFor(r) { it.serial > 0 && !it.busy() }; check(!s.appearance.dark() && s.locator == anchor) }
        println("PASS Kotlin/JNI palettes, shared persistent theme, high-density chrome, legacy names and unchanged progress/settings")
    }
}
