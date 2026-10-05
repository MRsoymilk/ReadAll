package xin.soymilk.readall

import java.io.File

object TocDragSmoke {
    @JvmStatic fun main(args: Array<String>) {
        val root = File(args[0]); val book = File(root, "toc.epub"); val font = File(root, "font.ttf"); val before = JniSmoke.hash(book); var cases = 0
        for (dense in arrayOf(false, true)) for (scenario in 0 until 4) {
            val state = File(root, "toc-drag-state-$dense-$scenario"); val expected = when (scenario) { 2 -> 1; 3 -> 21; else -> 2 }
            NativeReader(book.path, font.path, state.path, 400, 640, 16, 24, if (dense) 1080 else 400, if (dense) 1728 else 640).use { r ->
                val first = JniSmoke.waitFor(r) { it.serial > 0 && !it.busy() }; r.command(NativeReader.CONTENTS); JniSmoke.waitFor(r) { it.uiMode == "toc" && !it.busy() }
                val entries = r.contents(); check(entries.size == 30); val touch = TouchRouter(8f) { kind, x, y -> r.command(NativeReader.TOUCH + kind, x, y) }; touch.down(200, 260)
                when (scenario) {
                    0 -> { touch.move(200, 184); touch.up(200, 184, 0) }
                    1 -> { touch.move(200, 146); touch.move(200, 184); touch.up(200, 184, 0) }
                    2 -> { touch.move(200, 460); touch.move(200, 422); touch.up(200, 422, 0) }
                    else -> { touch.move(200, -9740); touch.move(200, -9740); touch.move(200, -9702); touch.up(200, -9702, 0) }
                }
                // Actual TOC geometry: y=136, height=298, max offset=842; partial rows stay clickable.
                touch.down(200, 148); touch.up(200, 148, 0); val jumped = JniSmoke.waitFor(r) { it.locator != first.locator && !it.busy() }
                check(jumped.position.contains("第 ${entries[expected].spine + 1}/30 章")) { "wrong chapter: $scenario dense=$dense ${jumped.position}" }
                check(jumped.uiMode == "expanded"); cases++
            }
        }
        check(before.contentEquals(JniSmoke.hash(book))); println("PASS $cases Kotlin TouchRouter/JNI TOC cases: direction, edge reversal, coalescing, density and chapter picking")
    }
}
