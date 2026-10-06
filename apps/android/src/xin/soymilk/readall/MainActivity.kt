@file:Suppress("DEPRECATION") // Keep the existing API-26 SAF and pre-33 back implementations.
package xin.soymilk.readall

import android.app.Activity
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.view.Choreographer
import android.view.Gravity
import android.view.View
import android.view.WindowInsets
import android.widget.*
import java.io.File
import java.nio.ByteBuffer
import java.util.Locale
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicReference

/** Android lifecycle/SAF services around the unchanged shared Rust reading presentation. */
class MainActivity : Activity(), ReaderView.Listener {
    private val ui = Handler(Looper.getMainLooper())
    private val io = Executors.newSingleThreadExecutor()
    private val pixels = Executors.newSingleThreadExecutor()
    private val spare = AtomicReference<Bitmap?>()
    private var pixelBuffer: ByteBuffer? = null // Owned exclusively by the pixel executor.
    @Volatile private var epoch = 0
    @Volatile private var importing = false
    @Volatile private var destroyed = false
    @Volatile private var imported = 0L
    @Volatile private var total = 0L
    private var resumed = false; private var frameScheduled = false; private var copyPending = false; private var remembered = false
    private var shownSerial = 0L
    private val loadingFeedback = LoadingFeedback()
    private lateinit var pageLoading: PageLoadingIndicator
    private var reader: NativeReader? = null
    private var lastState: NativeReader.State? = null
    private var currentFile: File? = null
    private var currentName = "ReadAll"; private var currentUri = ""; private var currentShelfId = ""
    private lateinit var page: ReaderView
    private lateinit var root: FrameLayout
    private lateinit var home: ShelfHome
    private lateinit var loading: LinearLayout
    private lateinit var shelf: ShelfController
    private lateinit var themeButton: Button
    private lateinit var appearance: NativeReader.Appearance
    private var themeBusy = false; private var appearanceEpoch = 0
    private lateinit var status: TextView
    private lateinit var progress: ProgressBar
    private lateinit var choreographer: Choreographer
    private val schedule = Runnable(::requestFrame)
    private val resizeTask = Runnable(::resizeNative)
    private val frames = Choreographer.FrameCallback { frameScheduled = false; poll() }
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState); choreographer = Choreographer.getInstance()
        val theme = getSharedPreferences("appearance", MODE_PRIVATE).getString("theme", "light") ?: "light"
        appearance = try { NativeReader.appearance(theme) } catch (_: IllegalStateException) { NativeReader.appearance("light") }
        root = FrameLayout(this).apply { setBackgroundColor(appearance.canvas) }
        if (Build.VERSION.SDK_INT >= 30) {
            window.setDecorFitsSystemWindows(false)
            root.setOnApplyWindowInsetsListener { view, insets ->
                val bars = insets.getInsets(WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout())
                view.setPadding(bars.left, bars.top, bars.right, maxOf(bars.bottom, insets.getInsets(WindowInsets.Type.ime()).bottom)); insets
            }
        }
        page = ReaderView(this, this); root.addView(page, FrameLayout.LayoutParams(-1, -1))
        shelf = ShelfController(this, io, appearance, object : ShelfController.Host {
            override fun pickBooks() = pick()
            override fun toggleTheme() = this@MainActivity.toggleTheme()
            override fun openBook(file: File, name: String, uri: String, id: String) = openShelfBook(file, name, uri, id)
        })
        home = shelf.view; themeButton = home.themeButton; root.addView(home, FrameLayout.LayoutParams(-1, -1))
        loading = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL; setPadding(dp(20), dp(16), dp(20), dp(16)); setBackgroundColor(appearance.panel) }
        status = text("准备打开", 14); loading.addView(status)
        progress = ProgressBar(this, null, android.R.attr.progressBarStyleHorizontal).apply { max = 1000 }; loading.addView(progress, LinearLayout.LayoutParams(-1, dp(5)))
        val actions = LinearLayout(this); loading.addView(actions); button(actions, "返回 / 取消", ::returnHome)
        button(actions, "重试") { if (currentUri.isNotEmpty()) importBook(Uri.parse(currentUri)) else openLast() }
        root.addView(loading, FrameLayout.LayoutParams(-1, -2, Gravity.CENTER).apply { setMargins(dp(20), 0, dp(20), 0) }); loading.visibility = View.GONE
        pageLoading = PageLoadingIndicator(this)
        root.addView(pageLoading, FrameLayout.LayoutParams(dp(22), dp(22), Gravity.BOTTOM or Gravity.RIGHT).apply { setMargins(0, 0, dp(12), dp(10)) })
        setContentView(root); applyAppearance(appearance); loadAppearance(); shelf.load()
        if (Build.VERSION.SDK_INT >= 33) onBackInvokedDispatcher.registerOnBackInvokedCallback(android.window.OnBackInvokedDispatcher.PRIORITY_DEFAULT, ::goBack)
        if (savedInstanceState?.getBoolean("reading", false) == true) page.post(::openLast)
    }
    private fun dp(value: Int) = Math.round(value * resources.displayMetrics.density)
    private fun text(value: String, size: Int) = TextView(this).apply { text = value; textSize = size.toFloat(); setTextColor(appearance.ink); setPadding(0, dp(8), 0, dp(8)) }
    private fun button(parent: LinearLayout, label: String, action: () -> Unit): Button {
        val button = Button(this).apply { text = label; isAllCaps = false; setOnClickListener { action() } }
        val horizontal = parent.orientation == LinearLayout.HORIZONTAL
        parent.addView(button, LinearLayout.LayoutParams(if (horizontal) 0 else -1, dp(48), if (horizontal) 1f else 0f)); return button
    }
    private fun stateDirectory() = File(filesDir, "reader-state").absolutePath
    private fun applyAppearance(value: NativeReader.Appearance) {
        appearance = value; AndroidTheme.apply(this, root, home, loading, page, progress, pageLoading, value); shelf.theme(value)
        val mirror = getSharedPreferences("appearance", MODE_PRIVATE)
        if (value.name != mirror.getString("theme", "")) mirror.edit().putString("theme", value.name).apply()
    }
    private fun loadAppearance() {
        val request = ++appearanceEpoch; val directory = stateDirectory()
        io.execute {
            try { val value = NativeReader.loadAppearance(directory); ui.post { if (!destroyed && request == appearanceEpoch && reader == null) applyAppearance(value) } }
            catch (error: Exception) { ui.post { if (!destroyed && request == appearanceEpoch && reader == null) showError(error) } }
        }
    }
    private fun toggleTheme() {
        if (themeBusy || reader != null || importing) return
        themeBusy = true; themeButton.isEnabled = false; val request = ++appearanceEpoch; val directory = stateDirectory(); val next = if (appearance.dark()) "light" else "dark"
        io.execute {
            try { val value = NativeReader.saveTheme(directory, next); ui.post { if (!destroyed) { themeBusy = false; themeButton.isEnabled = true; if (request == appearanceEpoch && reader == null) applyAppearance(value) } } }
            catch (error: Exception) { ui.post { if (!destroyed) { themeBusy = false; themeButton.isEnabled = true; showError(error) } } }
        }
    }
    private fun pick() {
        if (themeBusy || shelf.busy()) return
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).apply { addCategory(Intent.CATEGORY_OPENABLE); type = "*/*"; putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true); addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION) }
        startActivityForResult(intent, PICK_BOOK)
    }
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != PICK_BOOK || resultCode != RESULT_OK || data == null) return
        val selected = LinkedHashSet<Uri>(); val clip = data.clipData
        if (clip != null) { if (clip.itemCount > 64) { home.message("每批最多导入 64 本图书"); return }; repeat(clip.itemCount) { clip.getItemAt(it).uri?.let(selected::add) } }
        else data.data?.let(selected::add)
        for (uri in selected) try { contentResolver.takePersistableUriPermission(uri, data.flags and Intent.FLAG_GRANT_READ_URI_PERMISSION) } catch (_: SecurityException) { }
        shelf.importBooks(selected.toList())
    }
    private fun importBook(uri: Uri) {
        val preserve = currentFile; closeReader(); val token = ++epoch; importing = true; imported = 0; total = 0; currentUri = uri.toString(); home.visibility = View.GONE; showProgress("读取图书", 0, 0); requestFrame()
        io.execute {
            try {
                val importedBook = BookFiles.read(contentResolver, uri, shelf.booksDirectory(), preserve, { done, all -> if (token == epoch) { imported = done; total = all } }, { token != epoch || destroyed || Thread.currentThread().isInterrupted })
                val saved = shelf.accept(importedBook.file, importedBook.name, uri.toString()); val font = BookFiles.font(assets, File(filesDir, "fonts"))
                ui.post { if (token == epoch && !destroyed) { currentShelfId = saved.id; importing = false; startReader(importedBook.file, saved.label(), font, token) } }
            } catch (error: Exception) { ui.post { if (token == epoch && !destroyed) { importing = false; showError(error) } } }
        }
    }
    private fun openShelfBook(file: File, name: String, uri: String, id: String) {
        if (importing || themeBusy || destroyed) return
        closeReader(); val token = ++epoch; currentUri = uri; currentShelfId = id; importing = true; home.visibility = View.GONE; showProgress("准备打开图书", 0, 0); requestFrame()
        prepareFont(file, name, token)
    }
    private fun prepareFont(file: File, name: String, token: Int) {
        io.execute {
            try { val font = BookFiles.font(assets, File(filesDir, "fonts")); ui.post { if (token == epoch && !destroyed) { importing = false; startReader(file, name, font, token) } } }
            catch (error: Exception) { ui.post { if (token == epoch && !destroyed) { importing = false; showError(error) } } }
        }
    }
    private fun openLast() {
        if (importing || themeBusy) return
        if (shelf.openLast()) return
        val saved = getSharedPreferences("library", MODE_PRIVATE); val path = saved.getString("book", "").orEmpty(); val uri = saved.getString("uri", "").orEmpty(); val file = File(path)
        try {
            if (path.isEmpty() || !file.isFile || (!file.canonicalPath.startsWith(File(cacheDir, "books").canonicalPath + File.separator) && !file.canonicalPath.startsWith(shelf.booksDirectory().canonicalPath + File.separator))) { if (uri.isNotEmpty()) importBook(Uri.parse(uri)) else pick(); return }
        } catch (error: Exception) { showError(error); return }
        closeReader(); val token = ++epoch; currentUri = uri; currentShelfId = if (file.name.matches(Regex("[0-9a-f]{64}\\.(epub|mobi|pdf)"))) file.name.substring(0, 64) else ""
        importing = true; imported = 0; total = 0; home.visibility = View.GONE; showProgress("准备继续阅读", 0, 0); requestFrame()
        prepareFont(file, saved.getString("name", "图书") ?: "图书", token)
    }
    private fun startReader(file: File, name: String, font: File, token: Int) {
        if (token != epoch || destroyed) return
        if (page.width == 0 || page.height == 0) { page.post { startReader(file, name, font, token) }; return }
        try {
            val v = page.viewportSize(); reader = NativeReader(file.absolutePath, font.absolutePath, stateDirectory(), v[0], v[1], 20, 16, v[2], v[3])
            currentFile = file; currentName = name; shownSerial = 0; remembered = false; lastState = null; loadingFeedback.reset(); home.visibility = View.GONE; showProgress("准备正文", 0, 0); requestFrame()
        } catch (error: Throwable) { if (error !is Exception && error !is LinkageError) throw error; showError(error) }
    }
    override fun viewport(width: Int, height: Int) { ui.removeCallbacks(resizeTask); ui.postDelayed(resizeTask, 90) }
    private fun resizeNative() { val owner = reader ?: return; val v = page.viewportSize(); try { owner.viewport(v[0], v[1], v[2], v[3]); requestFrame() } catch (error: Exception) { showError(error) } }
    override fun action(code: Int, a: Int, b: Int) {
        val owner = reader ?: return; val last = lastState
        if (code in NativeReader.TOUCH..NativeReader.TOUCH + 9 && (last == null || last.serial == 0L || last.closed()) && code !in intArrayOf(NativeReader.TOUCH + 4, NativeReader.TOUCH + 7, NativeReader.TOUCH + 8)) return
        try { owner.command(code, a, b); requestFrame() } catch (error: Exception) { if (code < NativeReader.TOUCH) showError(error) }
    }
    override fun input(mode: String, text: String) { val owner = reader ?: return; try { owner.input(mode, text); requestFrame() } catch (error: Exception) { showError(error) } }
    private fun requestFrame() { ui.removeCallbacks(schedule); if (resumed && !destroyed && !frameScheduled) { frameScheduled = true; choreographer.postFrameCallback(frames) } }
    private fun poll() {
        if (!resumed || destroyed) return
        if (importing) { showProgress("读取图书", imported, total); ui.postDelayed(schedule, 80); return }
        val owner = reader ?: return
        try {
            val state = owner.state(); lastState = state
            if (state.closed()) { if (state.notice.isEmpty()) returnHome() else showError(IllegalStateException(state.notice)); return }
            updateLoading(state)
            if (state.serial > 0) {
                if (appearance.name != state.appearance.name) applyAppearance(state.appearance)
                if (!remembered) { remembered = true; getSharedPreferences("library", MODE_PRIVATE).edit().putString("book", checkNotNull(currentFile).absolutePath).putString("name", currentName).putString("uri", currentUri).apply(); shelf.progress(currentShelfId, state.percent) }
                page.state(state); if (state.serial != shownSerial && !copyPending) copyFrame(owner, state, epoch)
            }
            effects(owner)
            if (state.animating || page.touching() || copyPending) requestFrame() else ui.postDelayed(schedule, if (state.busy()) 60 else 80)
        } catch (error: Throwable) { if (error !is Exception && error !is LinkageError) throw error; showError(error) }
    }
    private fun recycle(image: Bitmap?) { if (image != null && !image.isRecycled) image.recycle() }
    private fun release(image: Bitmap?) = recycle(spare.getAndSet(image))
    private fun copyFrame(owner: NativeReader, state: NativeReader.State, token: Int) {
        copyPending = true
        pixels.execute {
            var image: Bitmap? = null; var problem: Throwable? = null
            try {
                val buffer = pixelBuffer?.takeIf { it.capacity() == state.byteLength() } ?: ByteBuffer.allocateDirect(state.byteLength()).also { pixelBuffer = it }
                if (owner.copyPixels(state, buffer)) {
                    image = spare.getAndSet(null)
                    if (image?.let { it.width != state.width || it.height != state.height } == true) { recycle(image); image = null }
                    val target = image ?: Bitmap.createBitmap(state.width, state.height, Bitmap.Config.ARGB_8888).also { it.density = Bitmap.DENSITY_NONE; image = it }
                    buffer.position(0); target.copyPixelsFromBuffer(buffer)
                }
            } catch (error: Throwable) { if (error !is Exception && error !is OutOfMemoryError) throw error; problem = error }
            val ready = image; val error = problem
            ui.post {
                if (token != epoch || destroyed || reader !== owner) recycle(ready)
                else {
                    copyPending = false
                    if (error != null) { recycle(ready); showError(error) }
                    else { if (ready != null) { release(page.picture(ready, state)); shownSerial = state.serial }; requestFrame() }
                }
            }
        }
    }
    private fun effects(owner: NativeReader) {
        val effects = owner.effects()
        for (n in effects.indices step 2) try {
            when (effects[n]) {
                "copy" -> { (getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager).setPrimaryClip(ClipData.newPlainText("ReadAll", effects[n + 1])); owner.hostReply(1, "") }
                "paste" -> { val clip = (getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager).primaryClip; val text = if (clip != null && clip.itemCount > 0) clip.getItemAt(0).text else null; owner.hostReply(2, text?.toString().orEmpty()) }
                "url" -> { val uri = Uri.parse(effects[n + 1]); require(uri.scheme == "https" || uri.scheme == "http") { "不支持的链接类型" }; startActivity(Intent(Intent.ACTION_VIEW, uri)); owner.hostReply(3, "") }
            }
        } catch (error: Exception) { try { owner.hostReply(0, error.message ?: "系统服务不可用") } catch (_: Exception) { } }
    }
    private fun updateLoading(state: NativeReader.State) {
        val feedback = loadingFeedback.update(shownSerial > 0, state.busy(), SystemClock.uptimeMillis())
        if (feedback == LoadingFeedback.INITIAL) { if (state.busy()) showProgress(state.phase, state.done, state.total) else showProgress("显示页面", 0, 0) }
        else { loading.visibility = View.GONE; pageLoading.show(feedback == LoadingFeedback.CORNER) }
    }
    private fun hidePageLoading() { loadingFeedback.reset(); pageLoading.show(false) }
    private fun showProgress(phase: String, done: Long, total: Long) {
        hidePageLoading(); loading.visibility = View.VISIBLE; progress.visibility = View.VISIBLE; progress.isIndeterminate = total <= 0
        if (total > 0) progress.progress = minOf(1000.0, 1000.0 * done / total).toInt()
        status.text = phase + if (total > 0) String.format(Locale.ROOT, " · %.1f%%", 100.0 * minOf(done, total) / total) else "…"
    }
    private fun showError(error: Throwable) { hidePageLoading(); loading.visibility = View.VISIBLE; progress.visibility = View.GONE; status.text = "ReadAll：${error.message ?: error.javaClass.simpleName}" }
    private fun closeReader() {
        lastState?.takeIf { it.serial > 0 }?.let { shelf.progress(currentShelfId, it.percent) }
        currentShelfId = ""; hidePageLoading(); shownSerial = 0; page.cancelTouch(); page.closeInput(); ui.removeCallbacks(schedule); ui.removeCallbacks(resizeTask)
        choreographer.removeFrameCallback(frames); frameScheduled = false; copyPending = false
        val owner = reader; reader = null; lastState = null; owner?.close()
    }
    private fun returnHome() { ++epoch; importing = false; closeReader(); release(page.picture(null)); loading.visibility = View.GONE; home.visibility = View.VISIBLE; loadAppearance(); shelf.resume() }
    private fun goBack() {
        if (reader == null && shelf.busy()) { shelf.cancelImport(); return }
        if (importing) { returnHome(); return }
        if (reader != null) { val state = lastState; if (state == null || state.serial == 0L || state.closed()) returnHome() else action(NativeReader.BACK, 0, 0); return }
        finish()
    }
    override fun onWindowFocusChanged(hasFocus: Boolean) { super.onWindowFocusChanged(hasFocus); if (hasFocus && ::appearance.isInitialized && ::pageLoading.isInitialized) { AndroidTheme.apply(this, root, home, loading, page, progress, pageLoading, appearance); shelf.theme(appearance) } }
    @Suppress("OVERRIDE_DEPRECATION") override fun onBackPressed() = goBack()
    override fun onResume() { super.onResume(); resumed = true; if (reader != null) action(NativeReader.PAUSE, 0, 0); requestFrame() }
    override fun onPause() { resumed = false; shelf.pause(); lastState?.takeIf { it.serial > 0 }?.let { shelf.progress(currentShelfId, it.percent) }; hidePageLoading(); page.cancelTouch(); ui.removeCallbacks(schedule); choreographer.removeFrameCallback(frames); frameScheduled = false; try { reader?.command(NativeReader.PAUSE, 1, 0) } catch (_: Exception) { }; super.onPause() }
    override fun onSaveInstanceState(outState: Bundle) { outState.putBoolean("reading", reader != null); super.onSaveInstanceState(outState) }
    override fun onDestroy() { destroyed = true; ++epoch; closeReader(); shelf.close(); io.shutdown(); pixels.shutdownNow(); release(null); super.onDestroy() }
    companion object { private const val PICK_BOOK = 41 }
}
