package xin.soymilk.readall

import android.app.Activity
import android.content.res.ColorStateList
import android.os.Build
import android.os.SystemClock
import android.text.Editable
import android.text.TextWatcher
import android.view.Gravity
import android.view.View
import android.view.accessibility.AccessibilityManager
import android.view.inputmethod.InputMethodManager
import android.widget.*

/** Native list/grid bookshelf chrome; geometry, view IDs and notice lifecycle remain unchanged. */
class ShelfHome(activity: Activity, covers: ShelfCoverCache, private var colors: NativeReader.Appearance, mode: Int, sort: String, private val callbacks: Callbacks) : LinearLayout(activity) {
    interface Callbacks : ShelfCanvas.Listener { fun add(); fun theme(); fun mode(mode: Int); fun sort(sort: String); fun cancelImport() }
    @JvmField val canvas = ShelfCanvas(activity, covers, colors, callbacks)
    @JvmField val themeButton: Button
    private val title: TextView; private val count: TextView; private val message: TextView
    private val resumeLabel: TextView; private val resumeTitle: TextView; private val resumePercent: TextView
    private val search: EditText
    private val searchIcon: ImageView; private val resumeIcon: ImageView; private val resumeArrow: ImageView
    private val add: Button; private val sortButton: Button; private val cancel: Button; private val clear: Button
    private lateinit var dismissNotice: Button
    private val notice = ShelfNotice()
    private val expireNotice = Runnable { notice.expire(SystemClock.uptimeMillis()); renderNotice() }
    private val searchBox: LinearLayout; private val segments: LinearLayout; private val continueCard: LinearLayout
    private lateinit var footer: LinearLayout
    private val importProgress: ProgressBar
    private val modes: Array<Button>
    private var books: List<ShelfStore.Book> = emptyList()
    private var mode = ShelfGeometry.mode(mode)
    private var sort = sort
    private var busy = false; private var compact = false
    init {
        orientation = VERTICAL; isFocusableInTouchMode = true
        val header = row().apply { setPadding(dp(20), dp(16), dp(20), dp(12)) }
        val heading = column(); title = text("书库", 30, true); count = text("正在读取…", 12, false); ShelfStyle.singleLine(count)
        heading.addView(title); heading.addView(count, LayoutParams(-1, -2).apply { topMargin = dp(6) }); header.addView(heading, LayoutParams(0, -2, 1f))
        themeButton = ShelfStyle.button(activity, "", callbacks::theme); header.addView(themeButton, LayoutParams(dp(48), dp(48)))
        add = ShelfStyle.button(activity, "导入", callbacks::add); header.addView(add, LayoutParams(dp(88), dp(48)).apply { leftMargin = dp(8) }); addView(header)
        searchBox = row().apply { setPadding(dp(14), 0, 0, 0) }; searchIcon = ImageView(activity); searchBox.addView(searchIcon, LayoutParams(dp(20), dp(20)))
        search = EditText(activity).apply {
            textSize = 15f; typeface = ShelfStyle.REGULAR; setSingleLine(true); hint = "搜索书名、作者或格式"; includeFontPadding = false
            setPadding(dp(10), 0, 0, 0); background = null; imeOptions = android.view.inputmethod.EditorInfo.IME_ACTION_SEARCH
            contentDescription = "搜索书库"; filters = arrayOf(android.text.InputFilter.LengthFilter(512))
        }
        searchBox.addView(search, LayoutParams(0, dp(52), 1f))
        clear = ShelfStyle.button(activity, "") { search.setText("") }; clear.contentDescription = "清空搜索"; searchBox.addView(clear, LayoutParams(dp(48), dp(52))); clear.visibility = INVISIBLE; addView(searchBox, outer(52, 0, 12))
        search.setOnEditorActionListener { _, _, _ -> clearSearch(); true }; search.setOnFocusChangeListener { _, _ -> searchBackground() }
        val controls = row(); segments = row().apply { setPadding(dp(3), dp(3), dp(3), dp(3)) }
        modes = Array(2) { i -> ShelfStyle.button(activity, if (i == 0) "列表" else "封面") { val anchor = canvas.focusId(); this.mode = i; callbacks.mode(i); update(anchor) }.also { segments.addView(it, LayoutParams(0, dp(48), 1f)) } }
        controls.addView(segments, LayoutParams(0, dp(54), 1f)); controls.addView(View(activity), LayoutParams(dp(12), 1))
        sortButton = ShelfStyle.button(activity, "最近") { this.sort = when (this.sort) { "recent" -> "title"; "title" -> "added"; else -> "recent" }; callbacks.sort(this.sort); update(null) }
        controls.addView(sortButton, LayoutParams(dp(88), dp(48))); addView(controls, outer(54, 0, 12))
        continueCard = row().apply { setPadding(dp(16), dp(12), dp(14), dp(12)); minimumHeight = dp(80); isFocusable = true; setOnClickListener { last()?.let(callbacks::open) } }
        resumeIcon = ImageView(activity); continueCard.addView(resumeIcon, LayoutParams(dp(24), dp(28)))
        val details = column(); resumeLabel = text("继续阅读", 12, false); resumeTitle = text("", 16, true); ShelfStyle.singleLine(resumeTitle)
        details.addView(resumeLabel); details.addView(resumeTitle, LayoutParams(-1, -2).apply { topMargin = dp(6) }); continueCard.addView(details, LayoutParams(0, -2, 1f).apply { leftMargin = dp(12); rightMargin = dp(8) })
        resumePercent = text("", 12, false); continueCard.addView(resumePercent, LayoutParams(-2, -2)); resumeArrow = ImageView(activity)
        continueCard.addView(resumeArrow, LayoutParams(dp(18), dp(18)).apply { leftMargin = dp(8) }); addView(continueCard, outer(-2, 0, 12)); continueCard.visibility = GONE
        addView(canvas, LayoutParams(-1, 0, 1f))
        footer = row().apply { setPadding(dp(12), 0, dp(4), 0); minimumHeight = dp(48) }
        importProgress = ProgressBar(activity, null, android.R.attr.progressBarStyleSmall); footer.addView(importProgress, LayoutParams(dp(18), dp(18))); importProgress.visibility = GONE
        message = text("", 12, false).apply { maxLines = 3; accessibilityLiveRegion = ACCESSIBILITY_LIVE_REGION_POLITE }
        footer.addView(message, LayoutParams(0, -2, 1f).apply { leftMargin = dp(8); topMargin = dp(8); bottomMargin = dp(8) })
        cancel = ShelfStyle.button(activity, "取消", callbacks::cancelImport); footer.addView(cancel, LayoutParams(dp(64), dp(48))); cancel.visibility = GONE
        dismissNotice = ShelfStyle.button(activity, "", ::clearNotice); dismissNotice.contentDescription = "关闭提示"; footer.addView(dismissNotice, LayoutParams(dp(48), dp(48))); dismissNotice.visibility = GONE
        addView(footer, outer(-2, 4, 8)); footer.visibility = GONE
        search.addTextChangedListener(object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) { }
            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) { clear.visibility = if (!s.isNullOrEmpty()) VISIBLE else INVISIBLE; update("") }
            override fun afterTextChanged(s: Editable?) { }
        })
        theme(colors)
    }
    private fun dp(value: Int) = ShelfStyle.dp(context, value)
    private fun outer(height: Int, top: Int, bottom: Int) = LayoutParams(-1, if (height < 0) height else dp(height)).apply { setMargins(dp(20), dp(top), dp(20), dp(bottom)) }
    private fun row() = LinearLayout(context).apply { orientation = HORIZONTAL; gravity = Gravity.CENTER_VERTICAL }
    private fun column() = LinearLayout(context).apply { orientation = VERTICAL }
    private fun text(value: String, size: Int, medium: Boolean) = ShelfStyle.text(context, value, size, colors.ink, medium)
    fun books(rows: List<ShelfStore.Book>, anchor: String?) { books = rows.toList(); update(anchor) }
    private fun last(): ShelfStore.Book? { var latest: ShelfStore.Book? = null; for (book in books) if (book.opened > 0 && (latest == null || book.opened > latest.opened)) latest = book; return latest }
    private fun update(anchor: String?) {
        val visible = ShelfStore.select(books, search.text.toString(), sort)
        count.text = "${books.size} 本图书" + if (visible.size != books.size) " · 找到 ${visible.size} 本" else " · 本地书库"
        canvas.empty(search.text.toString().trim { it <= ' ' }.isNotEmpty()); canvas.data(visible, mode, anchor); updateContinue()
        sortButton.text = when (sort) { "title" -> "书名"; "added" -> "导入"; else -> "最近" }
        sortButton.contentDescription = "排序：" + when (sort) { "title" -> "书名"; "added" -> "导入时间"; else -> "最近阅读" } + "，点击切换"
        styleTabs()
    }
    private fun updateContinue() {
        val latest = last(); continueCard.visibility = if (latest == null || compact) GONE else VISIBLE
        if (latest != null) { resumeTitle.text = latest.label(); resumePercent.text = latest.progress(); continueCard.contentDescription = "继续阅读，${latest.label()}，${latest.progress()}" }
    }
    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) { super.onSizeChanged(w, h, oldw, oldh); val small = h < dp(440); if (small != compact) { compact = small; count.visibility = if (small) GONE else VISIBLE; updateContinue() }; styleTabs() }
    fun busy(value: Boolean, status: String) { busy = value; add.isEnabled = !value; continueCard.isEnabled = !value; continueCard.alpha = if (value) .55f else 1f; cancel.visibility = if (value) VISIBLE else GONE; importProgress.visibility = if (value) VISIBLE else GONE; message(status) }
    fun busy() = busy
    fun message(value: String) {
        var timeout = ShelfNotice.TIMEOUT_MS; val access = context.getSystemService(Activity.ACCESSIBILITY_SERVICE) as? AccessibilityManager
        if (Build.VERSION.SDK_INT >= 29 && access != null) timeout = access.getRecommendedTimeoutMillis(timeout, AccessibilityManager.FLAG_CONTENT_TEXT or AccessibilityManager.FLAG_CONTENT_CONTROLS)
        notice.update(value, busy, SystemClock.uptimeMillis(), timeout); if (!isShown) notice.dismiss(); renderNotice()
    }
    private fun renderNotice() {
        removeCallbacks(expireNotice); notice.expire(SystemClock.uptimeMillis()); message.text = notice.text(); footer.visibility = if (notice.visible()) VISIBLE else GONE
        dismissNotice.visibility = if (notice.visible() && !notice.working()) VISIBLE else GONE
        val delay = notice.remaining(SystemClock.uptimeMillis()); if (isShown && delay > 0) postDelayed(expireNotice, delay)
    }
    fun clearNotice() { notice.dismiss(); renderNotice() }
    // Framework visibility callbacks can occur inside View's super-constructor.
    override fun onVisibilityChanged(changedView: View, visibility: Int) { super.onVisibilityChanged(changedView, visibility); if (::footer.isInitialized && ::dismissNotice.isInitialized) { if (!isShown) notice.dismiss(); renderNotice() } }
    override fun onWindowVisibilityChanged(visibility: Int) { super.onWindowVisibilityChanged(visibility); if (::footer.isInitialized && ::dismissNotice.isInitialized) { if (visibility != VISIBLE) notice.dismiss(); renderNotice() } }
    override fun onAttachedToWindow() { super.onAttachedToWindow(); renderNotice() }
    override fun onDetachedFromWindow() { removeCallbacks(expireNotice); notice.dismiss(); super.onDetachedFromWindow() }
    fun clearSearch() { search.clearFocus(); (context.getSystemService(Activity.INPUT_METHOD_SERVICE) as InputMethodManager).hideSoftInputFromWindow(search.windowToken, 0) }
    fun theme(p: NativeReader.Appearance) {
        colors = p; setBackgroundColor(p.canvas); title.setTextColor(p.ink); count.setTextColor(p.muted); message.setTextColor(p.muted)
        resumeLabel.setTextColor(p.muted); resumeTitle.setTextColor(p.ink); resumePercent.setTextColor(p.accent); search.setTextColor(p.ink); search.setHintTextColor(p.muted)
        searchBackground(); searchIcon.setImageDrawable(ShelfIcon(ShelfIcon.SEARCH, p.muted, dp(20))); canvas.theme(p)
        for (button in arrayOf(themeButton, sortButton, cancel, clear, dismissNotice)) ShelfStyle.buttonTheme(button, p, false)
        ShelfStyle.buttonTheme(add, p, true); themeButton.contentDescription = if (p.dark()) "切换到亮色主题" else "切换到暗色主题"; themeButton.tooltipText = themeButton.contentDescription
        ShelfStyle.icon(themeButton, if (p.dark()) ShelfIcon.SUN else ShelfIcon.MOON, p.ink, true); ShelfStyle.icon(add, ShelfIcon.ADD, p.onAccent, false)
        ShelfStyle.icon(sortButton, ShelfIcon.SORT, p.muted, false); ShelfStyle.icon(clear, ShelfIcon.CLOSE, p.muted, true); ShelfStyle.icon(dismissNotice, ShelfIcon.CLOSE, p.muted, true)
        continueCard.background = ShelfStyle.touch(context, p.panel, p.accent, 18); resumeIcon.setImageDrawable(ShelfIcon(ShelfIcon.BOOK, p.accent, dp(24))); resumeArrow.setImageDrawable(ShelfIcon(ShelfIcon.ARROW, p.muted, dp(18)))
        footer.background = ShelfStyle.shape(context, p.panel, 12); importProgress.indeterminateTintList = ColorStateList.valueOf(p.accent); styleTabs()
    }
    private fun searchBackground() { searchBox.background = ShelfStyle.shape(context, colors.panel, 16).apply { if (search.hasFocus()) setStroke(dp(1), this@ShelfHome.colors.accent) } }
    private fun styleTabs() {
        segments.background = ShelfStyle.shape(context, colors.panel, 14)
        for (i in modes.indices) {
            val selected = i == mode; val button = modes[i]; button.isSelected = selected; button.setTextColor(if (selected) colors.ink else colors.muted); button.backgroundTintList = null
            button.background = ShelfStyle.touch(context, if (selected) colors.selected else colors.panel, colors.accent, 11)
            if ((width == 0 || width >= dp(360)) && resources.configuration.fontScale <= 1.2f) ShelfStyle.icon(button, if (i == 0) ShelfIcon.LIST else ShelfIcon.GRID, if (selected) colors.accent else colors.muted, false)
            else { button.setCompoundDrawables(null, null, null, null); button.compoundDrawablePadding = 0 }
            button.contentDescription = (if (i == 0) "列表模式" else "封面网格模式") + if (selected) "，已选择" else ""
        }
    }
}
