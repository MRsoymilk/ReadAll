package xin.soymilk.readall

import java.io.*
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.util.LinkedHashMap
import java.util.Locale

/** Binary format, bounds, process mutex and atomic replacement are unchanged. */
class ShelfStore @Throws(IOException::class) constructor(root: File) {
    @JvmField val root: File = root.canonicalFile
    @JvmField val books = File(this.root, "books")
    @JvmField val covers = File(this.root, "covers")
    private val index = File(this.root, "shelf-v1.bin")
    init {
        if (!this.root.isDirectory && !this.root.mkdirs()) throw IOException("无法创建书库")
        if (!books.isDirectory && !books.mkdirs()) throw IOException("无法创建图书目录")
        if (!covers.isDirectory && !covers.mkdirs()) throw IOException("无法创建封面目录")
    }
    class Book @Throws(IOException::class) constructor(@JvmField val file: String, name: String?, title: String?, author: String?, format: String?, uri: String?, alias: String?, @JvmField val added: Long, @JvmField val opened: Long, @JvmField val percent: Double, @JvmField val pinned: Boolean) {
        @JvmField val id: String
        @JvmField val name: String; @JvmField val title: String; @JvmField val author: String
        @JvmField val format: String; @JvmField val uri: String; @JvmField val alias: String
        init {
            if (!file.matches(Regex("[0-9a-f]{64}\\.(epub|mobi|pdf)"))) throw IOException("书库文件标识无效")
            id = file.substring(0, 64); this.name = clean(name, 512); this.title = clean(title, 512); this.author = clean(author, 512)
            this.format = clean(format, 32); this.uri = clean(uri, 8192); this.alias = clean(alias, 512)
            if (added < 0 || opened < 0 || !percent.isFinite() || percent < -1 || percent > 100) throw IOException("书库阅读状态无效")
        }
        fun label() = alias.ifEmpty { title.ifEmpty { name } }
        fun progress() = if (percent < 0) { if (opened > 0) "待恢复进度" else "未读" } else String.format(Locale.ROOT, "%.1f%%", percent)
        @Throws(IOException::class) fun edit(alias: String, pinned: Boolean, opened: Long, percent: Double) = Book(file, name, title, author, format, uri, alias, added, opened, percent, pinned)
    }
    @Throws(IOException::class) fun bookFile(book: Book) = contained(books, book.file)
    @Throws(IOException::class) fun coverFile(book: Book) = contained(covers, book.id + ".png")
    @Throws(IOException::class) fun load(): List<Book> = synchronized(MUTEX) { read().values.toList() }
    private fun read(): LinkedHashMap<String, Book> {
        val rows = LinkedHashMap<String, Book>()
        if (!index.exists()) return rows
        if (index.length() > MAX_INDEX_BYTES) throw IOException("书库索引过大；原文件保留")
        DataInputStream(BufferedInputStream(FileInputStream(index))).use { input ->
            if (input.readInt() != MAGIC) throw IOException("无法识别书库索引；原文件保留")
            val count = input.readInt(); if (count !in 0..LIMIT) throw IOException("书库条目超限")
            repeat(count) {
                val book = Book(input.readUTF(), input.readUTF(), input.readUTF(), input.readUTF(), input.readUTF(), input.readUTF(), input.readUTF(), input.readLong(), input.readLong(), input.readDouble(), input.readBoolean())
                if (rows.put(book.id, book) != null) throw IOException("书库含重复标识")
            }
            if (input.read() != -1) throw IOException("书库索引包含未知数据")
        }
        return rows
    }
    private fun change(edit: (LinkedHashMap<String, Book>) -> Unit) = synchronized(MUTEX) {
        RandomAccessFile(File(root, "shelf.lock"), "rw").use { lockFile ->
            lockFile.channel.use { channel -> channel.lock().use { lock ->
                if (!lock.isValid) throw IOException("书库忙碌")
                val rows = read(); edit(rows)
                if (rows.size > LIMIT) throw IOException("书库最多保存 1000 本，请先移除不需要的记录")
                val temporary = File.createTempFile("shelf-", ".tmp", root)
                try {
                    FileOutputStream(temporary).use { file -> DataOutputStream(BufferedOutputStream(file)).use { out ->
                        out.writeInt(MAGIC); out.writeInt(rows.size)
                        for (book in rows.values) {
                            out.writeUTF(book.file); out.writeUTF(book.name); out.writeUTF(book.title); out.writeUTF(book.author); out.writeUTF(book.format); out.writeUTF(book.uri); out.writeUTF(book.alias)
                            out.writeLong(book.added); out.writeLong(book.opened); out.writeDouble(book.percent); out.writeBoolean(book.pinned)
                        }
                        out.flush(); file.fd.sync()
                    } }
                    if (temporary.length() > MAX_INDEX_BYTES) throw IOException("书库索引超过 16 MiB 上限；原记录保留")
                    Files.move(temporary.toPath(), index.toPath(), StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING)
                } finally { Files.deleteIfExists(temporary.toPath()) }
            } }
        }
    }
    @Throws(IOException::class) fun add(imported: Book) { change { rows ->
        val old = rows[imported.id]
        rows[imported.id] = Book(imported.file, imported.name, imported.title, imported.author, imported.format, imported.uri, old?.alias ?: "", old?.added ?: imported.added, old?.opened ?: 0, old?.percent ?: -1.0, old?.pinned ?: false)
    } }
    @Throws(IOException::class) fun remove(ids: Collection<String>) { change { rows -> ids.forEach { rows.remove(it) } } }
    @Throws(IOException::class) fun rename(id: String, title: String) { val alias = clean(title.trim { it <= ' ' }, 512); change { rows -> rows[id]?.let { rows[id] = it.edit(alias, it.pinned, it.opened, it.percent) } } }
    @Throws(IOException::class) fun pin(id: String, value: Boolean) { change { rows -> rows[id]?.let { rows[id] = it.edit(it.alias, value, it.opened, it.percent) } } }
    @Throws(IOException::class) fun opened(id: String, now: Long) { change { rows -> rows[id]?.let { rows[id] = it.edit(it.alias, it.pinned, maxOf(it.opened, now), it.percent) } } }
    @Throws(IOException::class) fun progress(id: String, percent: Double, now: Long) { change { rows -> rows[id]?.let { rows[id] = it.edit(it.alias, it.pinned, maxOf(it.opened, now), maxOf(0.0, minOf(100.0, percent))) } } }
    companion object {
        const val LIMIT = 1000; const val MAGIC = 0x52534c31; const val MAX_INDEX_BYTES = 16 * 1024 * 1024
        private val MUTEX = Any()
        @JvmStatic @Throws(IOException::class) fun clean(value: String?, max: Int): String { if (value == null || value.length > max || '\u0000' in value) throw IOException("书库文本过长或无效"); return value }
        private fun contained(parent: File, name: String): File { val file = File(parent, name).canonicalFile; if (file.parentFile != parent.canonicalFile) throw IOException("书库路径越界"); return file }
        @JvmStatic fun select(all: List<Book>, query: String, sort: String): List<Book> {
            val q = query.trim { it <= ' ' }.lowercase(Locale.ROOT)
            val visible = all.filter { q.isEmpty() || (it.label() + " " + it.author + " " + it.name + " " + it.format).lowercase(Locale.ROOT).contains(q) }
            val order: Comparator<Book> = when (sort) {
                "title" -> Comparator { a, b -> String.CASE_INSENSITIVE_ORDER.compare(a.label(), b.label()) }
                "added" -> compareByDescending { it.added }
                else -> compareByDescending { maxOf(it.opened, it.added) }
            }
            return visible.sortedWith(compareBy<Book> { !it.pinned }.then(order).thenBy { it.id })
        }
    }
}
