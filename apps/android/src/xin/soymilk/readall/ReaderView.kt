package xin.soymilk.readall

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.RectF
import android.os.Trace
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.VelocityTracker
import android.view.View
import android.view.ViewConfiguration
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import android.view.inputmethod.InputMethodManager

/** The entire reading surface still comes from Rust; this View only presents frames and input. */
class ReaderView(context: Context, private val listener: Listener) : View(context) {
    interface Listener { fun viewport(width: Int, height: Int); fun action(code: Int, a: Int, b: Int); fun input(mode: String, text: String) }
    private var bitmap: Bitmap? = null
    private var layoutWidth = 0; private var layoutHeight = 0; private var backgroundColor = 0
    private val paint = Paint(Paint.FILTER_BITMAP_FLAG)
    private val touch = TouchRouter(8f) { kind, x, y -> listener.action(NativeReader.TOUCH + kind, x, y) }
    private var velocity: VelocityTracker? = null
    private var state: NativeReader.State? = null
    private var connection: ReaderInput? = null
    private var ready = false
    private var editorMode = ""
    private val longPress = Runnable { if (ready) touch.longPress() }
    init {
        isFocusable = true; isFocusableInTouchMode = true
        // Retired bitmaps may be reused only after synchronous UI-thread drawing finishes.
        setLayerType(LAYER_TYPE_SOFTWARE, null)
        contentDescription = "ReadAll 阅读页面。滑动翻页，长按拖选文字；底部箭头展开工具栏和目录。"
    }
    private fun ime() = context.getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager
    fun background(color: Int) { backgroundColor = color; invalidate() }
    fun viewportSize() = ReaderViewport.sizes(maxOf(1, width), maxOf(1, height), resources.displayMetrics.density)
    @JvmOverloads fun picture(image: Bitmap?, frame: NativeReader.State? = null): Bitmap? {
        val old = bitmap; bitmap = image
        if (frame != null) { layoutWidth = frame.logicalWidth; layoutHeight = frame.logicalHeight }
        ready = image != null && layoutWidth > 0 && layoutHeight > 0; invalidate(); return old
    }
    fun touching() = touch.active()
    fun state(next: NativeReader.State) {
        state = next
        bitmap?.let { image ->
            val matches = next.logicalWidth == layoutWidth && next.logicalHeight == layoutHeight && next.width == image.width && next.height == image.height
            if (!matches && touch.active()) cancelTouch()
            ready = matches
        }
        val mode = if (next.editing) next.uiMode else ""
        if (mode != editorMode) {
            editorMode = mode; connection = null
            val ime = ime(); ime.restartInput(this)
            if (mode.isNotEmpty()) { requestFocus(); post { if (editorMode == mode) ime.showSoftInput(this, InputMethodManager.SHOW_IMPLICIT) } }
            else ime.hideSoftInputFromWindow(windowToken, 0)
        }
        connection?.nativeText(next.input)
    }
    fun cancelTouch() { removeCallbacks(longPress); touch.cancel(); velocity?.recycle(); velocity = null }
    fun closeInput() { editorMode = ""; connection = null; ime().hideSoftInputFromWindow(windowToken, 0) }
    private fun scale(): Float = bitmap?.let { minOf(width.toFloat() / it.width, height.toFloat() / it.height) } ?: 1f
    private fun point(x: Float, y: Float, image: Bitmap) = ReaderViewport.point(x, y, width, height, image.width, image.height, layoutWidth, layoutHeight)
    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) { super.onSizeChanged(w, h, oldw, oldh); cancelTouch(); if (w > 0 && h > 0) listener.viewport(w, h) }
    override fun onDraw(canvas: Canvas) {
        Trace.beginSection("ReadAll.present")
        try {
            super.onDraw(canvas); canvas.drawColor(backgroundColor)
            bitmap?.let { image ->
                val s = scale(); val w = image.width * s; val h = image.height * s
                paint.isFilterBitmap = image.width != width || image.height != height
                canvas.drawBitmap(image, null, RectF((width - w) / 2, (height - h) / 2, (width + w) / 2, (height + h) / 2), paint)
            }
        } finally { Trace.endSection() }
    }
    override fun onTouchEvent(event: MotionEvent): Boolean {
        if (!ready) return true
        if (event.pointerCount > 1 || event.actionMasked == MotionEvent.ACTION_CANCEL) { cancelTouch(); return true }
        val image = bitmap ?: return true
        val p = point(event.x, event.y, image)
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> { requestFocus(); cancelTouch(); velocity = VelocityTracker.obtain().also { it.addMovement(event) }; touch.down(p[0], p[1]); postDelayed(longPress, ViewConfiguration.getLongPressTimeout().toLong()) }
            MotionEvent.ACTION_MOVE -> { velocity?.addMovement(event); touch.move(p[0], p[1]) }
            MotionEvent.ACTION_UP -> {
                removeCallbacks(longPress); var fling = 0
                velocity?.let { tracker ->
                    tracker.addMovement(event); tracker.computeCurrentVelocity(1000)
                    val current = state
                    if (current != null && (current.uiMode == "toc" || current.pageMode == "scroll")) fling = Math.round(-tracker.yVelocity / scale() * layoutHeight / image.height * .15f)
                    tracker.recycle()
                }
                velocity = null; touch.up(p[0], p[1], fling); performClick()
                if (editorMode.isNotEmpty() && p[1] in 78 until 122) ime().showSoftInput(this, InputMethodManager.SHOW_IMPLICIT)
            }
        }
        return true
    }
    override fun performClick(): Boolean { super.performClick(); return true }
    override fun onCheckIsTextEditor() = editorMode.isNotEmpty()
    override fun onCreateInputConnection(outAttrs: EditorInfo): InputConnection? {
        val current = state ?: return null
        if (editorMode.isEmpty()) return null
        outAttrs.inputType = android.text.InputType.TYPE_CLASS_TEXT or if (editorMode == "note") android.text.InputType.TYPE_TEXT_FLAG_MULTI_LINE else 0
        outAttrs.imeOptions = (if (editorMode == "search") EditorInfo.IME_ACTION_SEARCH else EditorInfo.IME_ACTION_DONE) or EditorInfo.IME_FLAG_NO_EXTRACT_UI
        return ReaderInput(this, editorMode, current.input, { mode, text -> listener.input(mode, text) }, Runnable { listener.action(NativeReader.ACTIVATE, 0, 0); ime().hideSoftInputFromWindow(windowToken, 0) }).also { connection = it }
    }
    override fun onKeyDown(keyCode: Int, event: KeyEvent): Boolean {
        val code = if (event.isCtrlPressed) when (keyCode) { KeyEvent.KEYCODE_C -> NativeReader.COPY; KeyEvent.KEYCODE_V -> NativeReader.PASTE; else -> 0 }
        else when (keyCode) {
            KeyEvent.KEYCODE_F2 -> NativeReader.FIND; KeyEvent.KEYCODE_F3 -> NativeReader.ANNOTATIONS; KeyEvent.KEYCODE_F4 -> NativeReader.BOOKMARK
            KeyEvent.KEYCODE_F5 -> NativeReader.SETTINGS; KeyEvent.KEYCODE_F6 -> NativeReader.THEME; KeyEvent.KEYCODE_F7 -> NativeReader.NOTE; KeyEvent.KEYCODE_F8 -> NativeReader.HIGHLIGHT
            KeyEvent.KEYCODE_PAGE_DOWN, KeyEvent.KEYCODE_DPAD_DOWN, KeyEvent.KEYCODE_DPAD_RIGHT -> NativeReader.NEXT
            KeyEvent.KEYCODE_PAGE_UP, KeyEvent.KEYCODE_DPAD_UP, KeyEvent.KEYCODE_DPAD_LEFT -> NativeReader.PREVIOUS
            KeyEvent.KEYCODE_ESCAPE -> NativeReader.BACK; KeyEvent.KEYCODE_ENTER -> NativeReader.ACTIVATE; else -> 0
        }
        if (code != 0) { listener.action(code, 0, 0); return true }
        return super.onKeyDown(keyCode, event)
    }
}
