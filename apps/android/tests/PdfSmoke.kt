package xin.soymilk.readall

import java.io.ByteArrayOutputStream
import java.io.File
import java.util.Locale

/** Real host JVM/JNI fixed-page PDF reader regression. */
object PdfSmoke {
    private fun fixture(root: File): File {
        val book = File(root, "reader.pdf")
        val out = ByteArrayOutputStream()
        val offsets = ArrayList<Int>()
        fun write(text: String) = out.write(text.toByteArray(Charsets.ISO_8859_1))
        fun obj(number: Int, body: String) {
            offsets.add(out.size())
            write("$number 0 obj\n$body\nendobj\n")
        }
        fun stream(color: String): String {
            val body = "$color rg 0 0 200 300 re f\n"
            return "<< /Length ${body.toByteArray(Charsets.US_ASCII).size} >>\nstream\n${body}endstream"
        }
        write("%PDF-1.4\n")
        obj(1, "<< /Type /Catalog /Pages 2 0 R >>")
        obj(2, "<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 >>")
        obj(3, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 300] /Resources << >> /Contents 4 0 R >>")
        obj(4, stream("0.1 0.4 0.8"))
        obj(5, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Resources << >> /Contents 6 0 R >>")
        obj(6, stream("0.8 0.25 0.1"))
        obj(7, "<< /Title (Two Page PDF) /Author (ReadAll Test) >>")
        val xref = out.size()
        write("xref\n0 8\n0000000000 65535 f \n")
        offsets.forEach { write(String.format(Locale.ROOT, "%010d 00000 n \n", it)) }
        write("trailer\n<< /Size 8 /Root 1 0 R /Info 7 0 R >>\nstartxref\n$xref\n%%EOF\n")
        book.writeBytes(out.toByteArray())
        return book
    }

    @JvmStatic fun main(args: Array<String>) {
        val root = File(args[0]).canonicalFile
        val book = fixture(root)
        val font = File(root, "font.ttf")
        val state = File(root, "pdf-state")
        val before = JniSmoke.hash(book)
        var secondLocator = ""

        NativeReader(book.path, font.path, state.path, 400, 520, 16, 24).use { reader ->
            val first = JniSmoke.waitFor(reader) { it.serial > 0 && !it.busy() }
            JniSmoke.require(first.pageMode == "pdf", "PDF did not select fixed-page mode")
            JniSmoke.require(first.position.contains("1/2"), "wrong initial PDF page: ${first.position}")
            JniSmoke.require(first.locator.endsWith(":page-0"), "wrong initial PDF locator: ${first.locator}")
            JniSmoke.require(first.title == "Two Page PDF", "PDF title metadata not used")

            reader.command(NativeReader.CONTENTS)
            JniSmoke.waitFor(reader) { !it.busy() && reader.contents().size == 2 }
            val pages = reader.contents()
            JniSmoke.require(pages[0].title == "第 1 页" && pages[1].title == "第 2 页", "PDF page list is wrong")

            reader.command(NativeReader.NEXT)
            val second = JniSmoke.waitFor(reader) { it.locator.endsWith(":page-1") && !it.busy() }
            JniSmoke.require(second.position.contains("2/2"), "next did not move to PDF page 2")
            secondLocator = second.locator

            reader.command(NativeReader.FIND)
            JniSmoke.waitFor(reader) { it.notice.contains("全文搜索") }
            val theme = reader.state().appearance.name
            reader.command(NativeReader.THEME)
            JniSmoke.waitFor(reader) { !it.busy() && it.appearance.name != theme }

            reader.command(NativeReader.JUMP, 0, 0)
            JniSmoke.waitFor(reader) { it.locator.endsWith(":page-0") && !it.busy() }
            reader.command(NativeReader.JUMP, 1, 0)
            val saved = JniSmoke.waitFor(reader) { it.locator == secondLocator && !it.busy() }
            reader.command(NativeReader.SAVE)
            JniSmoke.waitFor(reader) { !it.busy() && it.locator == saved.locator }
        }

        Thread.sleep(100)
        NativeReader(book.path, font.path, state.path, 400, 520, 16, 24).use { reader ->
            val restored = JniSmoke.waitFor(reader) { it.serial > 0 && !it.busy() }
            JniSmoke.require(restored.locator == secondLocator, "saved PDF page did not restore")
        }
        JniSmoke.require(before.contentEquals(JniSmoke.hash(book)), "PDF source changed")
        println("PASS Kotlin/JVM JNI PDF: fixed pages, metadata, page list, navigation, unsupported tools, theme, progress restore and immutable source")
    }
}
