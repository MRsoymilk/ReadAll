package xin.soymilk.readall

import android.app.Activity
import android.app.Dialog
import android.graphics.Bitmap
import android.net.Uri
import android.os.Handler
import android.os.Looper
import java.io.*
import java.nio.ByteBuffer
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.security.MessageDigest
import java.util.Locale
import java.util.concurrent.ExecutorService

/** One IO owner performs mutations; UI callbacks carry generation checks and immutable records. */
class ShelfController(private val activity: Activity, private val io: ExecutorService, private var appearance: NativeReader.Appearance, private val host: Host) : ShelfHome.Callbacks {
    interface Host { fun pickBooks(); fun toggleTheme(); fun openBook(file: File, name: String, uri: String, id: String) }
    private val main = Handler(Looper.getMainLooper())
    private val directory = File(activity.filesDir, "bookshelf-v1")
    private val prefs = activity.getSharedPreferences("bookshelf", Activity.MODE_PRIVATE)
    private val cache: ShelfCoverCache
    @JvmField val view: ShelfHome
    private var store: ShelfStore? = null // IO executor only.
    private var preview: ByteBuffer? = null // IO executor only.
    private var books: List<ShelfStore.Book> = emptyList()
    @Volatile private var generation = 0
    @Volatile private var closed = false
    private var busy = false
    private var dialog: Dialog? = null
    private var mode: Int
    private var sort: String
    init {
        val saved = prefs.getInt("mode", ShelfGeometry.COVERS); mode = ShelfGeometry.mode(saved)
        if (saved != mode) prefs.edit().putInt("mode", mode).apply()
        sort = prefs.getString("sort", "recent") ?: "recent"
        cache = ShelfCoverCache(File(directory, "covers"), ::invalidate)
        view = ShelfHome(activity, cache, appearance, mode, sort, this)
    }
    private fun store(): ShelfStore = store ?: ShelfStore(directory).also { store = it }
    private fun invalidate() { if (!closed) view.canvas.invalidate() }
    fun load() { io.execute {
        try { val owner = store(); try { migrate() } catch (error: Exception) { error(error) }; val data = owner.load(); main.post { if (!closed) { books = data; view.books(data, prefs.getString("focus", "") ?: "") } } }
        catch (error: Exception) { error(error) }
    } }
    private fun reload() { try { val data = store().load(); main.post { if (!closed) { books = data; view.books(data, null) } } } catch (error: Exception) { error(error) } }
    private fun error(error: Throwable) { val token = generation; val message = error.message ?: error.javaClass.simpleName; main.post { if (!closed && token == generation) view.message("书库：$message") } }
    private fun migrate() {
        if (prefs.getBoolean("migrated-last-v1", false)) return
        val legacy = activity.getSharedPreferences("library", Activity.MODE_PRIVATE); val path = legacy.getString("book", "").orEmpty()
        if (path.isNotEmpty()) {
            val old = File(path).canonicalFile; val legacyRoot = File(activity.cacheDir, "books").canonicalFile
            if (old.isFile && old.parentFile == legacyRoot && old.name.matches(Regex("[0-9a-f]{64}\\.(epub|mobi)")) && old.length() <= BookFiles.MAX_BOOK) {
                val owner = store(); val target = File(owner.books, old.name); BookFiles.checkRoom(owner.books, target, old.length())
                if (!target.exists()) {
                    val temporary = File.createTempFile("migration-", ".tmp", owner.books)
                    try { Files.copy(old.toPath(), temporary.toPath(), StandardCopyOption.REPLACE_EXISTING); if (sha256(temporary) != old.name.substring(0, 64)) throw IOException("旧缓存标识不匹配；原文件保留"); Files.move(temporary.toPath(), target.toPath(), StandardCopyOption.ATOMIC_MOVE) }
                    finally { Files.deleteIfExists(temporary.toPath()) }
                }
                if (owner.load().none { it.file == target.name }) { val migrated = accept(target, legacy.getString("name", "图书").orEmpty(), legacy.getString("uri", "").orEmpty()); owner.opened(migrated.id, maxOf(1, old.lastModified())) }
            }
        }
        prefs.edit().putBoolean("migrated-last-v1", true).apply()
    }
    fun booksDirectory() = File(directory, "books")
    @Throws(Exception::class) fun accept(file: File, name: String, uri: String): ShelfStore.Book = accept(file, name, uri) { false }
    private fun accept(file: File, name: String, uri: String, cancelled: BookFiles.Cancelled): ShelfStore.Book {
        if (cancelled.get()) throw IOException("导入已取消")
        val owner = store()
        if (file.canonicalFile.parentFile != owner.books.canonicalFile) throw IOException("只能登记应用管理的图书副本")
        val buffer = preview ?: ByteBuffer.allocateDirect(NativeReader.PREVIEW_BYTES).also { preview = it }
        val p = NativeReader.preview(file.absolutePath, buffer)
        if (cancelled.get()) throw IOException("导入已取消")
        val book = ShelfStore.Book(file.name, safe(name, 512), safe(p.title, 512), safe(p.author, 512), p.format, safe(uri, 8192), "", System.currentTimeMillis(), 0, -1.0, false)
        try {
            if (p.width > 0) {
                val image = Bitmap.createBitmap(p.width, p.height, Bitmap.Config.ARGB_8888)
                val temporary = File.createTempFile("cover-", ".tmp", owner.covers)
                try {
                    buffer.position(0); image.copyPixelsFromBuffer(buffer)
                    FileOutputStream(temporary).use { out -> if (!image.compress(Bitmap.CompressFormat.PNG, 100, out)) throw IOException("无法写入封面"); out.fd.sync() }
                    Files.move(temporary.toPath(), owner.coverFile(book).toPath(), StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING)
                } finally { image.recycle(); Files.deleteIfExists(temporary.toPath()) }
            }
        } catch (_: IOException) { error(IOException("封面缓存不可用，将使用文字封面")) } catch (_: OutOfMemoryError) { error(IOException("封面缓存不可用，将使用文字封面")) }
        if (cancelled.get()) throw IOException("导入已取消")
        owner.add(book); main.post { if (!closed) cache.refresh(book.id) }; return book
    }
    fun importBooks(uris: List<Uri>) {
        if (busy || uris.isEmpty()) return
        busy = true; val token = ++generation; view.busy(true, "准备导入 ${uris.size} 本图书…")
        io.execute {
            var succeeded = 0; var failed = 0; var lastError = ""
            try {
                val owner = store()
                for ((i, uri) in uris.withIndex()) {
                    if (closed || token != generation) break
                    val number = i + 1; var last = 0L
                    try {
                        val result = BookFiles.read(activity.contentResolver, uri, owner.books, null, { done, _ ->
                            val now = System.nanoTime()
                            if (now - last > 80000000L) { last = now; main.post { if (!closed && token == generation) view.message("导入 $number / ${uris.size} · ${done / 1024 / 1024} MiB") } }
                        }, { closed || token != generation || Thread.currentThread().isInterrupted })
                        if (closed || token != generation) break
                        main.post { if (!closed && token == generation) view.message("读取书名与封面 · $number / ${uris.size}") }
                        accept(result.file, result.name, uri.toString()) { closed || token != generation || Thread.currentThread().isInterrupted }; succeeded++; reload()
                    } catch (error: Throwable) {
                        if (error !is Exception && error !is LinkageError && error !is OutOfMemoryError) throw error
                        if (token != generation) break
                        failed++; lastError = safe(error.message, 160)
                    }
                }
                val done = succeeded; val errors = failed; val why = lastError
                main.post { if (!closed && token == generation) { busy = false; view.busy(false, "已加入 $done 本" + if (errors > 0) "，失败 $errors 本：$why" else " · 重复图书自动合并") } }
            } catch (error: Exception) { val why = safe(error.message, 160); main.post { if (!closed && token == generation) { busy = false; view.busy(false, "导入未完成，已有图书保留：$why") } } }
        }
    }
    fun busy() = busy
    override fun cancelImport() { if (!busy) return; ++generation; busy = false; view.busy(false, "导入已取消，已完成的图书保留") }
    override fun add() { if (!busy) { view.clearNotice(); host.pickBooks() } }
    override fun theme() { if (!busy) host.toggleTheme() }
    override fun mode(mode: Int) { this.mode = ShelfGeometry.mode(mode); prefs.edit().putInt("mode", this.mode).apply() }
    override fun sort(sort: String) { this.sort = sort; prefs.edit().putString("sort", sort).apply() }
    override fun focused(id: String) { prefs.edit().putString("focus", id).apply() }
    override fun open(book: ShelfStore.Book) {
        if (busy) { view.message("正在导入；可先取消导入再阅读"); return }
        view.clearSearch(); view.canvas.stop()
        io.execute { try { val file = store().bookFile(book); if (!file.isFile) throw IOException("副本已丢失，请重新导入此书；进度和标注保留"); main.post { if (!closed && !busy) host.openBook(file, book.label(), book.uri, book.id) } } catch (error: Exception) { error(error) } }
    }
    fun openLast(): Boolean { var last: ShelfStore.Book? = null; for (book in books) if (last == null || book.opened > last.opened) last = book; val book = last ?: return false; open(book); return true }
    override fun menu(book: ShelfStore.Book) {
        if (busy || closed || activity.isFinishing) return
        view.clearSearch(); view.canvas.stop(); dialog?.dismiss()
        dialog = ShelfDialogs.actions(activity, appearance, book) { which ->
            if (!closed) when (which) {
                0 -> open(book)
                1 -> change { store().pin(book.id, !book.pinned) }
                2 -> { dialog = ShelfDialogs.rename(activity, appearance, book) { title -> change { store().rename(book.id, title) } } }
                3 -> change { accept(store().bookFile(book), book.name, book.uri) }
                else -> { dialog = ShelfDialogs.remove(activity, appearance, book) { purge -> change {
                    val owner = store(); owner.remove(setOf(book.id)); val legacy = activity.getSharedPreferences("library", Activity.MODE_PRIVATE)
                    if (File(legacy.getString("book", "").orEmpty()).name == book.file) legacy.edit().remove("book").remove("uri").remove("name").apply()
                    if (purge) try { Files.deleteIfExists(owner.bookFile(book).toPath()); Files.deleteIfExists(owner.coverFile(book).toPath()) } catch (_: IOException) { error(IOException("记录已移除，但副本清理失败")) }
                    main.post { if (!closed) cache.refresh(book.id) }
                } } }
            }
        }
    }
    private fun change(operation: () -> Unit) { io.execute { try { operation(); reload(); main.post { if (!closed) view.message("书库已更新") } } catch (error: Exception) { error(error) } } }
    fun progress(id: String?, percent: String) {
        if (id.isNullOrEmpty()) return
        val value = percent.toDoubleOrNull() ?: return; if (!value.isFinite()) return
        io.execute { try { store().progress(id, value, System.currentTimeMillis()) } catch (error: Exception) { error(error) } }
    }
    fun theme(value: NativeReader.Appearance) { appearance = value; view.theme(value) }
    fun resume() = load()
    fun pause() { view.clearNotice(); view.canvas.stop() }
    fun close() { closed = true; ++generation; dialog?.dismiss(); dialog = null; view.clearNotice(); cache.close(); view.canvas.stop() }
    companion object {
        private fun sha256(file: File): String {
            val digest = MessageDigest.getInstance("SHA-256"); FileInputStream(file).use { input -> val bytes = ByteArray(65536); while (true) { val n = input.read(bytes); if (n < 0) break; digest.update(bytes, 0, n) } }
            return digest.digest().joinToString("") { String.format(Locale.ROOT, "%02x", it.toInt() and 255) }
        }
        @JvmStatic fun safe(value: String?, limit: Int): String { if (value == null) return ""; val s = value.replace('\u0000', ' '); if (s.length <= limit) return s; val end = if (Character.isHighSurrogate(s[limit - 1])) limit - 1 else limit; return s.substring(0, end) }
    }
}
