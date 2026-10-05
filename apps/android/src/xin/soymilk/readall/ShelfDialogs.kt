package xin.soymilk.readall

import android.app.Activity
import android.app.Dialog
import android.content.Context
import android.content.res.ColorStateList
import android.graphics.Color
import android.graphics.drawable.ColorDrawable
import android.text.InputFilter
import android.text.TextUtils
import android.view.ContextThemeWrapper
import android.view.Gravity
import android.view.View
import android.view.Window
import android.view.WindowManager
import android.widget.*
import java.util.function.Consumer
import java.util.function.IntConsumer

/** Native bottom panels; destructive storage choices remain explicit and unchecked by default. */
class ShelfDialogs private constructor(private val activity: Activity, private val colors: NativeReader.Appearance, title: String, subtitle: String) {
    private val context: Context = ContextThemeWrapper(activity, if (colors.dark()) android.R.style.Theme_Material_Dialog_NoActionBar else android.R.style.Theme_Material_Light_Dialog_NoActionBar)
    private val dialog = Dialog(context)
    private val panel = LinearLayout(context)
    init {
        dialog.requestWindowFeature(Window.FEATURE_NO_TITLE)
        panel.orientation = LinearLayout.VERTICAL; panel.setPadding(dp(20), dp(16), dp(20), dp(20)); panel.background = ShelfStyle.shape(context, colors.panel, 24)
        val handle = View(context).apply { background = ShelfStyle.shape(context, colors.border, 2) }
        panel.addView(handle, LinearLayout.LayoutParams(dp(32), dp(4)).apply { gravity = Gravity.CENTER_HORIZONTAL; bottomMargin = dp(20) })
        panel.addView(ShelfStyle.text(context, title, 21, colors.ink, true).apply { maxLines = 3; ellipsize = TextUtils.TruncateAt.END })
        if (subtitle.isNotEmpty()) panel.addView(ShelfStyle.text(context, subtitle, 14, colors.muted, false).apply { setLineSpacing(dp(3).toFloat(), 1f) }, LinearLayout.LayoutParams(-1, -2).apply { topMargin = dp(10); bottomMargin = dp(14) })
        val scroll = object : ScrollView(context) {
            override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
                val limit = (resources.displayMetrics.heightPixels * .82f).toInt()
                val available = MeasureSpec.getSize(heightMeasureSpec).let { if (it > 0) it else limit }
                super.onMeasure(widthMeasureSpec, MeasureSpec.makeMeasureSpec(minOf(limit, available), MeasureSpec.AT_MOST))
            }
        }
        scroll.isFillViewport = false; scroll.clipToPadding = false; scroll.addView(panel); dialog.setContentView(scroll); dialog.setCanceledOnTouchOutside(true)
    }
    private fun dp(value: Int) = ShelfStyle.dp(context, value)
    private fun action(label: String, icon: Int, run: () -> Unit) {
        val row = ShelfStyle.button(context, label) { dialog.dismiss(); run() }
        ShelfStyle.buttonTheme(row, colors, false); ShelfStyle.icon(row, icon, colors.muted, false)
        row.compoundDrawablePadding = dp(14); row.gravity = Gravity.START or Gravity.CENTER_VERTICAL; row.setPadding(dp(8), 0, dp(8), 0)
        panel.addView(row, LinearLayout.LayoutParams(-1, dp(52)))
    }
    private fun buttons(primary: String, run: () -> Unit) {
        val row = LinearLayout(context).apply { orientation = LinearLayout.HORIZONTAL }
        val cancel = ShelfStyle.button(context, "取消", dialog::dismiss); val confirm = ShelfStyle.button(context, primary) { dialog.dismiss(); run() }
        ShelfStyle.buttonTheme(cancel, colors, false); ShelfStyle.buttonTheme(confirm, colors, true)
        row.addView(cancel, LinearLayout.LayoutParams(0, dp(48), 1f)); row.addView(confirm, LinearLayout.LayoutParams(0, dp(48), 1f).apply { leftMargin = dp(12) })
        panel.addView(row, LinearLayout.LayoutParams(-1, -2).apply { topMargin = dp(18) })
    }
    @Suppress("DEPRECATION") // Dialog windows retain API-26 keyboard resizing; the Activity uses modern insets.
    private fun show(): Dialog {
        dialog.show()
        dialog.window?.let { window ->
            window.setBackgroundDrawable(ColorDrawable(Color.TRANSPARENT)); window.setDimAmount(.28f); window.addFlags(WindowManager.LayoutParams.FLAG_DIM_BEHIND)
            window.setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE); window.setGravity(Gravity.BOTTOM or Gravity.CENTER_HORIZONTAL)
            window.attributes = window.attributes.apply { width = minOf(activity.resources.displayMetrics.widthPixels - dp(24), dp(560)); height = -2; y = dp(12) }
        }
        return dialog
    }
    companion object {
        @JvmStatic fun actions(activity: Activity, colors: NativeReader.Appearance, book: ShelfStore.Book, action: IntConsumer): Dialog {
            val sheet = ShelfDialogs(activity, colors, book.label(), (if (book.author.isEmpty()) book.format else book.author + " · " + book.format) + " · " + book.progress())
            val labels = arrayOf("打开阅读", if (book.pinned) "取消置顶" else "置顶图书", "修改显示书名", "重新读取封面", "移出书库")
            val icons = intArrayOf(ShelfIcon.BOOK, ShelfIcon.PIN, ShelfIcon.EDIT, ShelfIcon.REFRESH, ShelfIcon.TRASH)
            for (i in labels.indices) {
                if (i == 4) sheet.panel.addView(View(sheet.context).apply { setBackgroundColor(colors.border) }, LinearLayout.LayoutParams(-1, 1).apply { topMargin = sheet.dp(8); bottomMargin = sheet.dp(8) })
                sheet.action(labels[i], icons[i]) { action.accept(i) }
            }
            return sheet.show()
        }
        @JvmStatic fun rename(activity: Activity, colors: NativeReader.Appearance, book: ShelfStore.Book, save: Consumer<String>): Dialog {
            val sheet = ShelfDialogs(activity, colors, "修改显示书名", "只修改书库中的名称，不改动原书。留空恢复原书名。")
            val input = EditText(sheet.context).apply {
                setText(book.label()); setSingleLine(true); textSize = 16f; setTextColor(colors.ink); setHintTextColor(colors.muted); hint = "书名"
                setSelectAllOnFocus(true); filters = arrayOf(InputFilter.LengthFilter(512)); setPadding(sheet.dp(14), sheet.dp(10), sheet.dp(14), sheet.dp(10))
                background = ShelfStyle.touch(sheet.context, colors.canvas, colors.accent, 12); minimumHeight = sheet.dp(52); contentDescription = "显示书名"
            }
            sheet.panel.addView(input, LinearLayout.LayoutParams(-1, -2)); sheet.buttons("保存") { save.accept(input.text.toString()) }; return sheet.show()
        }
        @JvmStatic fun remove(activity: Activity, colors: NativeReader.Appearance, book: ShelfStore.Book, remove: Consumer<Boolean>): Dialog {
            val sheet = ShelfDialogs(activity, colors, "移出书库？", book.label() + "\n\n不会删除系统中的原书，也不会清除阅读进度、书签、高亮或笔记。")
            val clear = CheckBox(sheet.context).apply {
                text = "同时清理应用内副本和封面"; textSize = 14f; setTextColor(colors.ink)
                buttonTintList = ColorStateList(arrayOf(intArrayOf(android.R.attr.state_checked), intArrayOf()), intArrayOf(colors.accent, colors.muted))
                setPadding(0, sheet.dp(8), 0, sheet.dp(8)); minimumHeight = sheet.dp(48); isChecked = false
            }
            sheet.panel.addView(clear); sheet.buttons("移出书库") { remove.accept(clear.isChecked) }; return sheet.show()
        }
    }
}
