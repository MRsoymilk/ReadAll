package xin.soymilk.readall

import android.content.ContentResolver
import android.content.res.AssetManager
import android.net.Uri
import android.provider.OpenableColumns
import java.io.File
import java.io.FileOutputStream
import java.io.IOException
import java.io.RandomAccessFile
import java.security.MessageDigest
import java.util.Locale

/** Reads only caller-granted content URIs; never derives filesystem paths from a URI. */
object BookFiles {
    const val MAX_BOOK = 128L * 1024 * 1024
    fun interface Progress { fun update(done: Long, total: Long) }
    fun interface Cancelled { fun get(): Boolean }
    class Imported(@JvmField val file: File, @JvmField val name: String)
    @JvmStatic @Throws(Exception::class)
    fun read(resolver: ContentResolver, uri: Uri, cache: File, @Suppress("UNUSED_PARAMETER") preserve: File?, progress: Progress, cancelled: Cancelled): Imported {
        if (uri.scheme != "content") throw IOException("请选择系统文件选择器中的图书")
        var name = "图书"; var total = 0L
        resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE), null, null, null)?.use { cursor ->
            if (cursor.moveToFirst()) {
                val n = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME); val s = cursor.getColumnIndex(OpenableColumns.SIZE)
                if (n >= 0 && !cursor.isNull(n)) name = cursor.getString(n)
                if (s >= 0 && !cursor.isNull(s)) total = maxOf(0, cursor.getLong(s))
            }
        }
        if (name.length > 512) name = name.substring(0, 512)
        if (total > MAX_BOOK) throw IOException("图书超过 128 MiB 读取上限")
        if (!cache.isDirectory && !cache.mkdirs()) throw IOException("无法创建图书缓存")
        val temporary = File.createTempFile("import-", ".tmp", cache); var moved = false
        try {
            val digest = MessageDigest.getInstance("SHA-256"); var done = 0L; val block = ByteArray(65536)
            (resolver.openInputStream(uri) ?: throw IOException("文档提供程序没有返回内容")).use { input ->
                FileOutputStream(temporary).use { out ->
                    while (true) {
                        if (cancelled.get()) throw IOException("导入已取消")
                        val count = input.read(block); if (count < 0) break; if (count == 0) continue
                        done += count; if (done > MAX_BOOK) throw IOException("图书超过 128 MiB 读取上限")
                        digest.update(block, 0, count); out.write(block, 0, count); progress.update(done, total)
                    }
                    out.fd.sync()
                }
            }
            if (cancelled.get()) throw IOException("导入已取消")
            val extension = RandomAccessFile(temporary, "r").use { input ->
                if (input.length() < 4) throw IOException("文件过短，不是有效图书")
                if (input.readInt() == 0x504b0304) ".epub" else {
                    if (input.length() < 68) throw IOException("当前支持 EPUB、MOBI 和 AZW3")
                    input.seek(60); val magic = ByteArray(8); input.readFully(magic)
                    if (!magic.contentEquals("BOOKMOBI".toByteArray(Charsets.US_ASCII))) throw IOException("当前支持 EPUB、MOBI 和 AZW3")
                    ".mobi" // Native detection distinguishes MOBI6/7 and KF8, preserving stored IDs.
                }
            }
            val hex = digest.digest().joinToString("") { String.format(Locale.ROOT, "%02x", it.toInt() and 255) }
            val result = File(cache, hex + extension); checkRoom(cache, result, temporary.length())
            if (result.isFile) { if (!temporary.delete()) throw IOException("无法清理重复导入缓存") }
            else if (!temporary.renameTo(result)) throw IOException("无法完成图书缓存写入")
            moved = true; result.setLastModified(System.currentTimeMillis())
            return Imported(result, name)
        } finally { if (!moved) temporary.delete() }
    }
    @JvmStatic @Throws(IOException::class) fun checkRoom(directory: File, current: File, incoming: Long) {
        if (current.isFile) return
        val files = directory.listFiles { file -> file.isFile && file.name.matches(Regex("[0-9a-f]{64}\\.(epub|mobi)")) } ?: throw IOException("无法检查图书存储")
        var used = incoming; for (file in files) used += file.length()
        if (used > 2L * 1024 * 1024 * 1024) throw IOException("应用内图书副本达到 2 GiB 上限；没有自动删除任何图书")
    }
    @JvmStatic @Throws(IOException::class) fun font(assets: AssetManager, directory: File): File {
        if (!directory.isDirectory && !directory.mkdirs()) throw IOException("无法创建字体目录")
        val output = File(directory, "LXGWWenKaiLite-Regular.ttf")
        if (output.isFile && output.length() == 13872424L) return output
        val temporary = File.createTempFile("font-", ".tmp", directory); var installed = false
        try {
            assets.open("LXGWWenKaiLite-Regular.ttf").use { input -> FileOutputStream(temporary).use { out ->
                val bytes = ByteArray(65536); var total = 0L
                while (true) { val n = input.read(bytes); if (n < 0) break; total += n; if (total > 16 * 1024 * 1024) throw IOException("字体资源过大"); out.write(bytes, 0, n) }
                out.fd.sync()
                if (total != 13872424L) throw IOException("内置字体资源不完整")
                if (!temporary.renameTo(output)) throw IOException("无法安装内置字体")
                installed = true; return output
            } }
        } finally { if (!installed) temporary.delete() }
    }
}
