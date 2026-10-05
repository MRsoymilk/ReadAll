package xin.soymilk.readall

import java.io.File
import java.nio.ByteBuffer

object ReaderMenuSmoke {
    private fun send(r: NativeReader, code: Int, x: Int = 0, y: Int = 0): NativeReader.State { val serial = r.state().serial; r.command(code, x, y); return JniSmoke.waitFor(r) { it.serial > serial && !it.busy() } }
    @JvmStatic fun main(args: Array<String>) {
        val root = File(args[0])
        for (dense in arrayOf(false, true)) NativeReader(File(root, "book.epub").path, File(root, "font.ttf").path, File(root, "menu-state-$dense").path, 400, 640, 20, 16, if (dense) 1080 else 400, if (dense) 1728 else 640).use { r ->
            val initial = JniSmoke.waitFor(r) { it.serial > 0 && !it.busy() }; val anchor = initial.locator
            send(r, NativeReader.PAUSE, 1, 0); send(r, NativeReader.THEME); send(r, NativeReader.THEME)
            val settings = send(r, NativeReader.SETTINGS); check(settings.uiMode == "settings" && "↑" !in settings.notice)
            val image = ByteBuffer.allocateDirect(settings.byteLength()); check(r.copyPixels(settings, image)); val top = Math.round(40f * settings.height / 640f)
            check(ThemeSmoke.color(image, settings.width / 2, top, settings.width) == settings.appearance.panel)
            check(ThemeSmoke.color(image, Math.round(12f * settings.width / 400f), top, settings.width) != settings.appearance.panel)
            send(r, NativeReader.TOUCH + 1, 45, 186); check("size=20\n" in File(root, "menu-state-$dense/library-v1/settings.conf").readText())
            send(r, NativeReader.TOUCH + 1, 348, 186); send(r, NativeReader.TOUCH + 1, 300, 186); check(r.state().locator == anchor)
            send(r, NativeReader.TOUCH + 1, 366, 58); check(r.state().uiMode == "expanded")
            val find = send(r, NativeReader.FIND); check("Enter" !in find.notice); val serial = find.serial; r.input("search", "AAAA"); JniSmoke.waitFor(r) { it.serial > serial && !it.busy() }
            val result = send(r, NativeReader.TOUCH + 1, 344, 98); check("个结果" in result.notice)
            send(r, NativeReader.BACK); check("Ctrl" !in send(r, NativeReader.NOTE).notice); send(r, NativeReader.DISMISS); send(r, NativeReader.CONTENTS); check(r.state().uiMode == "toc")
        }
        println("PASS Kotlin/JNI modern menus: rounded frames, density, explicit controls, search/note/TOC and unchanged anchors")
    }
}
