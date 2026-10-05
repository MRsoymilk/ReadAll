package xin.soymilk.readall

import android.content.Context
import android.graphics.*
import android.os.Bundle
import android.view.*
import android.view.accessibility.AccessibilityNodeInfo
import android.widget.OverScroller

/** Virtualized list/grid bookshelf: only visible cards are drawn; both modes scroll vertically. */
class ShelfCanvas(context: Context, private val covers: ShelfCoverCache, private var colors: NativeReader.Appearance, private val listener: Listener) : View(context) {
    interface Listener { fun open(book: ShelfStore.Book); fun menu(book: ShelfStore.Book); fun focused(id: String) }
    private val density = resources.displayMetrics.density
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG or Paint.FILTER_BITMAP_FLAG)
    private val coverClip = Path()
    private var moreIcon = ShelfIcon(ShelfIcon.MORE, colors.ink, 20)
    private var bookIcon = ShelfIcon(ShelfIcon.BOOK, colors.accent, 28)
    private var pinIcon = ShelfIcon(ShelfIcon.PIN, colors.accent, 14)
    private var pressed = -1
    private val scroller = OverScroller(context)
    private val gestures: GestureDetector
    private val hits = ArrayList<Hit>()
    private var books: List<ShelfStore.Book> = emptyList()
    private var offset = 0f
    private var mode = ShelfGeometry.COVERS
    private var suppressTap = false
    private var pendingFocus: String? = null
    private var focused = ""
    private var emptyTitle = "书架还没有图书"
    private var emptyNote = "点击右上角「导入」，添加你的第一本书"
    private class Hit(val index: Int, val bounds: RectF, val menu: RectF) { fun contains(x: Float, y: Float) = bounds.contains(x, y) }
    init {
        isFocusable = true; contentDescription = "书库"
        gestures = GestureDetector(context, object : GestureDetector.SimpleOnGestureListener() {
            override fun onDown(e: MotionEvent): Boolean { suppressTap = !scroller.isFinished; stop(); val hit = hit(e.x / density, e.y / density); pressed = if (suppressTap) -1 else hit?.index ?: -1; invalidate(); return true }
            override fun onScroll(e1: MotionEvent?, e2: MotionEvent, distanceX: Float, distanceY: Float): Boolean { pressed = -1; offset = clamp(offset + distanceY / density); postInvalidateOnAnimation(); return true }
            override fun onFling(e1: MotionEvent?, e2: MotionEvent, velocityX: Float, velocityY: Float): Boolean {
                scroller.fling(0, Math.round(offset * density), 0, Math.round((-velocityY).coerceIn(-12000f, 12000f)), 0, 0, 0, Math.round(max() * density)); postInvalidateOnAnimation(); return true
            }
            override fun onSingleTapUp(e: MotionEvent): Boolean {
                if (suppressTap) return true
                val hit = hit(e.x / density, e.y / density) ?: return true
                if (hit.menu.contains(e.x / density, e.y / density)) listener.menu(books[hit.index]) else listener.open(books[hit.index])
                performClick(); return true
            }
            override fun onLongPress(e: MotionEvent) { hit(e.x / density, e.y / density)?.let { stop(); listener.menu(books[it.index]) } }
        })
    }
    fun width() = maxOf(1f, width / density)
    fun height() = maxOf(1f, height / density)
    private fun max() = ShelfGeometry.maxOffset(width(), height(), books.size, mode)
    private fun clamp(value: Float) = ShelfGeometry.clamp(value, max())
    fun theme(value: NativeReader.Appearance) { colors = value; moreIcon = ShelfIcon(ShelfIcon.MORE, value.ink, 20); bookIcon = ShelfIcon(ShelfIcon.BOOK, value.accent, 28); pinIcon = ShelfIcon(ShelfIcon.PIN, value.accent, 14); invalidate() }
    fun empty(filtering: Boolean) { emptyTitle = if (filtering) "没有找到匹配的图书" else "书架还没有图书"; emptyNote = if (filtering) "尝试其他书名、作者或格式" else "点击右上角「导入」，添加你的第一本书" }
    fun data(values: List<ShelfStore.Book>, requested: Int, anchor: String?) {
        val old = anchor ?: pendingFocus ?: focusId(); val previousOffset = offset; val previousMode = mode
        books = values.toList(); mode = ShelfGeometry.mode(requested); stop()
        offset = clamp(ShelfGeometry.restoreOffset(width(), mode, find(old), width(), previousMode, previousOffset))
        pendingFocus = if (width > 0) null else old; hits.clear(); invalidate(); if (pendingFocus == null) notifyFocus()
    }
    private fun find(id: String?) = books.indexOfFirst { it.id == id }
    fun focusId(): String { pendingFocus?.let { return it }; return books.getOrNull(ShelfGeometry.first(width(), books.size, mode, offset))?.id.orEmpty() }
    fun stop() { scroller.forceFinished(true); if (pressed != -1) { pressed = -1; invalidate() } }
    private fun notifyFocus() {
        val id = focusId()
        if (id != focused) { focused = id; listener.focused(id); val i = ShelfGeometry.first(width(), books.size, mode, offset); contentDescription = books.getOrNull(i)?.let { "${it.label()}，${i + 1} / ${books.size}，点击阅读，长按管理" } ?: "书库为空" }
    }
    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
        stop(); val at = pendingFocus?.let(::find) ?: ShelfGeometry.first(maxOf(1f, oldw / density), books.size, mode, offset)
        offset = clamp(ShelfGeometry.restoreOffset(maxOf(1f, w / density), mode, at, maxOf(1f, oldw / density), mode, if (oldw > 0) offset else 0f))
        pendingFocus = null; hits.clear(); invalidate(); notifyFocus()
    }
    override fun onTouchEvent(event: MotionEvent): Boolean {
        if (event.pointerCount > 1) { val cancel = MotionEvent.obtain(event); cancel.action = MotionEvent.ACTION_CANCEL; gestures.onTouchEvent(cancel); cancel.recycle(); stop(); return true }
        gestures.onTouchEvent(event)
        if (event.actionMasked == MotionEvent.ACTION_UP) { pressed = -1; invalidate(); if (scroller.isFinished) notifyFocus() }
        if (event.actionMasked == MotionEvent.ACTION_CANCEL) { stop(); notifyFocus() }
        return true
    }
    override fun performClick(): Boolean { super.performClick(); return true }
    override fun computeScroll() { if (scroller.computeScrollOffset()) { offset = clamp(scroller.currY / density); postInvalidateOnAnimation(); if (scroller.isFinished) notifyFocus() } }
    override fun onDetachedFromWindow() { stop(); super.onDetachedFromWindow() }
    override fun onGenericMotionEvent(event: MotionEvent): Boolean {
        if (event.action == MotionEvent.ACTION_SCROLL) { stop(); offset = clamp(offset - event.getAxisValue(MotionEvent.AXIS_VSCROLL) * 52); invalidate(); notifyFocus(); return true }
        return super.onGenericMotionEvent(event)
    }
    override fun onKeyDown(keyCode: Int, event: KeyEvent): Boolean {
        when (keyCode) { KeyEvent.KEYCODE_DPAD_LEFT, KeyEvent.KEYCODE_DPAD_UP -> move(-1); KeyEvent.KEYCODE_DPAD_RIGHT, KeyEvent.KEYCODE_DPAD_DOWN -> move(1); KeyEvent.KEYCODE_ENTER, KeyEvent.KEYCODE_DPAD_CENTER -> openFocused(false); else -> return super.onKeyDown(keyCode, event) }
        return true
    }
    private fun move(direction: Int) { stop(); offset = clamp(offset + direction * height() * .8f); invalidate(); notifyFocus() }
    private fun openFocused(menu: Boolean) { books.firstOrNull { it.id == focusId() }?.let { if (menu) listener.menu(it) else listener.open(it) } }
    override fun onInitializeAccessibilityNodeInfo(info: AccessibilityNodeInfo) {
        super.onInitializeAccessibilityNodeInfo(info); info.isScrollable = true
        for (action in arrayOf(AccessibilityNodeInfo.AccessibilityAction.ACTION_SCROLL_FORWARD, AccessibilityNodeInfo.AccessibilityAction.ACTION_SCROLL_BACKWARD, AccessibilityNodeInfo.AccessibilityAction.ACTION_CLICK, AccessibilityNodeInfo.AccessibilityAction.ACTION_LONG_CLICK)) info.addAction(action)
    }
    override fun performAccessibilityAction(action: Int, arguments: Bundle?): Boolean {
        when (action) { AccessibilityNodeInfo.ACTION_SCROLL_FORWARD -> move(1); AccessibilityNodeInfo.ACTION_SCROLL_BACKWARD -> move(-1); AccessibilityNodeInfo.ACTION_CLICK -> openFocused(false); AccessibilityNodeInfo.ACTION_LONG_CLICK -> openFocused(true); else -> return super.performAccessibilityAction(action, arguments) }
        return true
    }
    private fun hit(x: Float, y: Float): Hit? { for (i in hits.indices.reversed()) if (hits[i].contains(x, y)) return hits[i]; return null }
    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas); val save = canvas.save(); canvas.scale(density, density); canvas.drawColor(colors.canvas); hits.clear()
        if (books.isEmpty()) {
            val y = maxOf(0f, height() * .28f - 24)
            if (height() >= 170) { round(canvas, RectF(width() / 2 - 32, y, width() / 2 + 32, y + 64), 20f, colors.panel); icon(canvas, bookIcon, width() / 2 - 14, y + 18, 28); centered(canvas, emptyTitle, y + 98, width() - 40, 20f, colors.ink); centered(canvas, emptyNote, y + 126, width() - 40, 13f, colors.muted) }
            else { centered(canvas, emptyTitle, 32f, width() - 40, 18f, colors.ink); centered(canvas, emptyNote, 57f, width() - 40, 12f, colors.muted) }
        } else flat(canvas)
        canvas.restoreToCount(save)
    }
    private fun flat(c: Canvas) {
        val w = width(); val cw = ShelfGeometry.cellWidth(w); val first = ShelfGeometry.first(w, books.size, mode, offset); val end = ShelfGeometry.end(w, height(), books.size, mode, offset)
        for (i in first until end) {
            val b = books[i]; val t = ShelfGeometry.Tile(w, mode, i, offset); val x = t.left; val y = t.top
            val rect = RectF(x, y, t.right, t.bottom); val menu = RectF(t.menuLeft, t.menuTop, t.menuRight, t.menuBottom)
            if (i == pressed) round(c, rect, 12f, colors.selected)
            if (mode == ShelfGeometry.LIST) {
                cover(c, b, RectF(x, y + 8, x + 56, y + 88)); val tx = x + 72
                label(c, b.label(), tx, y + 28, t.right - tx - 48, 16f, colors.ink, true)
                label(c, b.author.ifEmpty { b.name }, tx, y + 51, t.right - tx - 8, 12f, colors.muted)
                label(c, (if (b.pinned) "置顶 · " else "") + b.format, tx, y + 75, (t.right - tx) * .55f, 11f, colors.muted)
                right(c, b.progress(), t.right - 8, y + 75, (t.right - tx) * .45f, 11f, colors.accent)
                bar(c, tx, y + 89, t.right - tx - 8, b.percent); paint.color = colors.border; c.drawRect(tx, y + 103, t.right, y + 103.5f, paint)
            } else {
                val base = y + cw * ShelfGeometry.COVER_RATIO; cover(c, b, RectF(x, y, x + cw, base))
                titleLines(c, b.label(), x, base + 23, cw, 15f); label(c, b.format, x, base + 64, cw * .45f, 11f, colors.muted)
                right(c, b.progress(), t.right, base + 64, cw * .55f, 11f, colors.accent); bar(c, x, base + 73, cw, b.percent)
                round(c, RectF(menu.centerX() - 15, menu.centerY() - 15, menu.centerX() + 15, menu.centerY() + 15), 15f, colors.panel)
                if (b.pinned) { round(c, RectF(x + 8, y + 9, x + 32, y + 33), 8f, colors.panel); icon(c, pinIcon, x + 13, y + 14, 14) }
            }
            icon(c, moreIcon, menu.centerX() - 10, menu.centerY() - 10, 20); hits.add(Hit(i, rect, menu))
        }
        val maximum = max()
        if (maximum > 0) { val sh = minOf(height(), maxOf(26f, height() * height() / (maximum + height()))); val sy = (height() - sh) * offset / maximum; round(c, RectF(w - 4, sy, w - 2, sy + sh), 1f, colors.border) }
    }
    private fun cover(c: Canvas, book: ShelfStore.Book, r: RectF) {
        val image = covers.get(book.id); val save = c.save(); coverClip.rewind(); coverClip.addRoundRect(r, 8f, 8f, Path.Direction.CW); c.clipPath(coverClip); round(c, r, 8f, colors.panel)
        if (image != null) {
            paint.color = Color.WHITE; c.drawRect(r, paint); val scale = minOf(r.width() / image.width, r.height() / image.height); val ww = image.width * scale; val hh = image.height * scale
            c.drawBitmap(image, null, RectF(r.centerX() - ww / 2, r.centerY() - hh / 2, r.centerX() + ww / 2, r.centerY() + hh / 2), paint)
        } else {
            paint.color = colors.accent; c.drawRect(r.left, r.top, r.left + maxOf(3f, r.width() * .045f), r.bottom, paint)
            val pad = r.width() * .12f; val size = minOf(23f, r.width() * .13f); var text = book.label()
            label(c, book.format, r.left + pad, r.top + r.height() * .17f, r.width() - pad * 2, maxOf(6f, size * .55f), colors.muted)
            for (line in 0 until 3) {
                if (text.isEmpty()) break
                paint.typeface = ShelfStyle.MEDIUM; paint.textSize = size
                var n = paint.breakText(text, true, r.width() - pad * 2, null)
                if (n > 0 && n < text.length && Character.isHighSurrogate(text[n - 1])) n--
                if (n <= 0) break
                label(c, text.substring(0, n), r.left + pad, r.top + r.height() * .41f + line * size * 1.4f, r.width() - pad * 2, size, colors.ink); text = text.substring(n)
            }
            label(c, "READALL", r.left + pad, r.bottom - r.height() * .10f, r.width() - pad * 2, maxOf(6f, size * .48f), colors.muted)
        }
        paint.color = (colors.border and 0x00ffffff) or 0x85000000.toInt(); paint.style = Paint.Style.STROKE; paint.strokeWidth = .7f; c.drawRoundRect(r, 8f, 8f, paint); paint.style = Paint.Style.FILL; c.restoreToCount(save)
    }
    private fun round(c: Canvas, r: RectF, radius: Float, color: Int) { paint.color = color; paint.style = Paint.Style.FILL; c.drawRoundRect(r, radius, radius, paint) }
    private fun bar(c: Canvas, x: Float, y: Float, w: Float, progress: Double) { round(c, RectF(x, y, x + w, y + 2), 1f, colors.border); if (progress >= 0) round(c, RectF(x, y, x + w * (progress / 100).toFloat(), y + 2), 1f, colors.accent) }
    private fun fit(text: String, width: Float): String {
        if (paint.measureText(text) <= width) return text
        var n = paint.breakText(text, true, maxOf(0f, width - paint.measureText("…")), null)
        if (n > 0 && n < text.length && Character.isHighSurrogate(text[n - 1])) n--
        return text.substring(0, n) + "…"
    }
    private fun label(c: Canvas, text: String, x: Float, y: Float, width: Float, size: Float, color: Int, medium: Boolean = false) {
        paint.typeface = if (medium) ShelfStyle.MEDIUM else ShelfStyle.REGULAR; paint.textSize = size; paint.color = color; paint.style = Paint.Style.FILL
        c.drawText(fit(text.replace('\n', ' '), maxOf(0f, width)), x, y, paint)
    }
    private fun right(c: Canvas, text: String, right: Float, y: Float, width: Float, size: Float, color: Int) { paint.typeface = ShelfStyle.REGULAR; paint.textSize = size; val value = fit(text, maxOf(0f, width)); label(c, value, right - paint.measureText(value), y, width, size, color) }
    private fun centered(c: Canvas, text: String, y: Float, width: Float, size: Float, color: Int) { paint.typeface = ShelfStyle.REGULAR; paint.textSize = size; val value = fit(text, maxOf(0f, width)); label(c, value, (width() - paint.measureText(value)) / 2, y, width, size, color) }
    private fun titleLines(c: Canvas, value: String, x: Float, y: Float, width: Float, size: Float) {
        val text = value.replace('\n', ' '); paint.typeface = ShelfStyle.MEDIUM; paint.textSize = size; var n = paint.breakText(text, true, maxOf(0f, width), null)
        if (n > 0 && n < text.length && Character.isHighSurrogate(text[n - 1])) n--
        if (n == 0) return
        label(c, text.substring(0, n), x, y, width, size, colors.ink, true)
        if (n < text.length) label(c, text.substring(n).trim { it <= ' ' }, x, y + 21, width, size, colors.ink, true)
    }
    private fun icon(c: Canvas, icon: ShelfIcon, x: Float, y: Float, size: Int) { val save = c.save(); c.translate(x, y); icon.setBounds(0, 0, size, size); icon.draw(c); c.restoreToCount(save) }
}
