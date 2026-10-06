package xin.soymilk.readall

import java.io.IOException
import java.nio.file.Files
import java.util.Locale
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

/** Real disk mutation, locking, corruption and layout regression; all paths are test-owned. */
object ShelfStoreSmoke {
    private fun book(n: Int, title: String, time: Long) = ShelfStore.Book(String.format(Locale.ROOT, "%064x.epub", n), "$title.epub", title, "作者", "EPUB", "content://fixture/$n", "", time, 0, -1.0, false)
    @JvmStatic fun main(args: Array<String>) {
        val tmp = Files.createTempDirectory("readall-shelf-test-")
        try {
            val a = ShelfStore(tmp.toFile()); val b = ShelfStore(tmp.toFile()); val one = book(1, "中文书籍😀", 1); val two = book(2, "Second book", 2)
            val pdf = ShelfStore.Book(String.format(Locale.ROOT, "%064x.pdf", 3), "paper.pdf", "PDF Paper", "Author", "PDF", "content://fixture/pdf", "", 3, 0, -1.0, false)
            check(pdf.file.endsWith(".pdf") && pdf.format == "PDF" && a.bookFile(pdf).name == pdf.file)
            a.add(one); a.add(two); check(a.load().size == 2); a.opened(one.id, 10); val migrated = b.load()[0]
            check(migrated.opened == 10L && migrated.percent == -1.0 && migrated.progress() == "待恢复进度")
            a.rename(one.id, "自定义标题"); a.pin(one.id, true); a.progress(one.id, 42.5, 20); a.add(book(1, "原书更新", 4))
            val restored = b.load()[0]; check(restored.label() == "自定义标题" && restored.percent == 42.5 && restored.pinned && restored.added == 1L)
            check(ShelfStore.select(b.load(), "自定义", "title").size == 1); check(ShelfStore.select(b.load(), "作者", "recent").size == 2); check(ShelfStore.select(b.load(), "", "recent")[0].id == one.id)
            a.bookFile(one).writeBytes(byteArrayOf(1, 2, 3)); Files.write(tmp.resolve("notes.keep"), byteArrayOf(7)); a.remove(setOf(one.id)); b.progress(one.id, 90.0, 30)
            check(a.load().size == 1); check(a.bookFile(one).exists() && Files.exists(tmp.resolve("notes.keep")))
            val workers = Executors.newFixedThreadPool(2)
            try { val x = workers.submit { for (i in 10 until 20) a.add(book(i, "A $i", i.toLong())) }; val y = workers.submit { for (i in 20 until 30) b.add(book(i, "B $i", i.toLong())) }; x.get(10, TimeUnit.SECONDS); y.get(10, TimeUnit.SECONDS) } finally { workers.shutdownNow() }
            check(a.load().size == 21); val index = tmp.resolve("shelf-v1.bin"); val valid = Files.readAllBytes(index); val bad = "foreign index".toByteArray(); Files.write(index, bad)
            JniSmoke.expect<IOException>("corrupt index overwritten") { a.add(one) }; check(Files.readAllBytes(index).contentEquals(bad)); Files.write(index, valid); check(ShelfStore(tmp.toFile()).load().size == 21)
            JniSmoke.expect<IOException>("path traversal accepted") { ShelfStore.Book("../outside.epub", "x", "x", "x", "EPUB", "", "", 0, 0, -1.0, false) }
            JniSmoke.expect<IOException>("NaN progress accepted") { a.progress(two.id, Double.NaN, 2) }
            for (width in floatArrayOf(256f, 320f, 393f, 848f, 1200f)) for (height in floatArrayOf(128f, 256f, 848f)) for (count in intArrayOf(0, 1, 5, 1000)) for (mode in ShelfGeometry.LIST..ShelfGeometry.COVERS) {
                val max = ShelfGeometry.maxOffset(width, height, count, mode); check(max >= 0 && max.isFinite())
                for (offset in floatArrayOf(0f, max / 2, max)) {
                    val first = ShelfGeometry.first(width, count, mode, offset); val end = ShelfGeometry.end(width, height, count, mode, offset)
                    check(first >= 0 && end >= first && end <= count)
                    val columns = if (mode == ShelfGeometry.LIST) 1 else ShelfGeometry.columns(width)
                    check(end - first <= (Math.ceil((height / ShelfGeometry.pitch(width, mode)).toDouble()).toInt() + 2) * columns)
                }
            }
            for (oldWidth in floatArrayOf(1f, 320f, 393f, 848f)) for (width in floatArrayOf(320f, 393f, 848f)) for (from in 0..1) for (to in 0..1) {
                val anchor = 23; val old = ShelfGeometry.restoreOffset(oldWidth, from, anchor, oldWidth, from, 0f); val offset = ShelfGeometry.restoreOffset(width, to, anchor, oldWidth, from, old)
                val first = ShelfGeometry.first(width, 1000, to, offset); check(offset.isFinite() && offset >= 0)
                check(first <= anchor && anchor < first + if (to == ShelfGeometry.LIST) 1 else ShelfGeometry.columns(width))
            }
            for (width in floatArrayOf(256f, 320f, 393f, 600f, 848f, 1200f)) for (mode in 0..1) for (i in 0 until 30) {
                val tile = ShelfGeometry.Tile(width, mode, i, 17.5f); val columns = if (mode == ShelfGeometry.LIST) 1 else ShelfGeometry.columns(width)
                check(tile.left >= ShelfGeometry.EDGE - .01f && tile.right <= width - ShelfGeometry.EDGE + .01f)
                check(tile.menuRight - tile.menuLeft >= 48 && tile.menuBottom - tile.menuTop >= 48)
                check(tile.menuLeft >= tile.left && tile.menuTop >= tile.top && tile.menuRight <= tile.right && tile.menuBottom <= tile.bottom)
                val following = ShelfGeometry.Tile(width, mode, i + columns, 17.5f); check(following.top > tile.bottom)
                val moved = ShelfGeometry.Tile(width, mode, i, 18.5f); check(Math.abs(tile.top - moved.top - 1) < .001f && Math.abs(tile.menuTop - moved.menuTop - 1) < .001f)
            }
            check(ShelfGeometry.columns(320f) == 2 && ShelfGeometry.columns(393f) == 2)
            check(ShelfGeometry.restoreOffset(393f, ShelfGeometry.COVERS, -1, 1f, ShelfGeometry.COVERS, 999f) == 0f)
            check(ShelfGeometry.LIST == 0 && ShelfGeometry.COVERS == 1 && ShelfGeometry.mode(0) == 0 && ShelfGeometry.mode(1) == 1)
            for (saved in intArrayOf(2, -1, 99, Int.MIN_VALUE, Int.MAX_VALUE)) {
                val next = ShelfGeometry.mode(saved); check(next == ShelfGeometry.COVERS)
                for (width in floatArrayOf(256f, 320f, 393f, 848f)) for (anchor in intArrayOf(0, 1, 23, 999)) { val offset = ShelfGeometry.restoreOffset(width, next, anchor, width, next, 0f); val first = ShelfGeometry.first(width, 1000, next, offset); check(first <= anchor && anchor < first + ShelfGeometry.columns(width)) }
            }
            check(Files.readAllBytes(index).contentEquals(valid))
            println("PASS Kotlin bookshelf: atomic storage, deduplication, alias/pin/search/sort, progress, safe removal, corruption, concurrency, bounded geometry, 48-unit targets and legacy modes")
        } finally { Files.walk(tmp).use { paths -> paths.sorted(Comparator.reverseOrder()).forEach { Files.deleteIfExists(it) } } }
    }
}
