package xin.soymilk.readall

import android.content.Context
import android.content.res.ColorStateList
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.graphics.drawable.RippleDrawable
import android.graphics.drawable.StateListDrawable
import android.text.TextUtils
import android.view.Gravity
import android.widget.Button
import android.widget.TextView

/** Small framework-only components; the Rust palette remains the color authority. */
object ShelfStyle {
    @JvmField val REGULAR: Typeface = Typeface.create("sans-serif", Typeface.NORMAL)
    @JvmField val MEDIUM: Typeface = Typeface.create("sans-serif-medium", Typeface.NORMAL)
    @JvmStatic fun dp(context: Context, value: Float) = Math.round(value * context.resources.displayMetrics.density)
    fun dp(context: Context, value: Int) = dp(context, value.toFloat())
    @JvmStatic fun shape(context: Context, fill: Int, radius: Float) = GradientDrawable().apply { setColor(fill); cornerRadius = dp(context, radius).toFloat() }
    fun shape(context: Context, fill: Int, radius: Int) = shape(context, fill, radius.toFloat())
    @JvmStatic fun touch(context: Context, fill: Int, ink: Int, radius: Float): RippleDrawable {
        val normal = shape(context, fill, radius); val focus = shape(context, fill, radius).apply { setStroke(dp(context, 2), ink) }
        val content = StateListDrawable().apply { addState(intArrayOf(android.R.attr.state_focused), focus); addState(intArrayOf(), normal) }
        return RippleDrawable(ColorStateList.valueOf((ink and 0x00ffffff) or 0x24000000), content, shape(context, -1, radius))
    }
    fun touch(context: Context, fill: Int, ink: Int, radius: Int) = touch(context, fill, ink, radius.toFloat())
    @JvmStatic fun button(context: Context, label: String, action: Runnable) = Button(context).apply {
        text = label; textSize = 14f; typeface = MEDIUM; isAllCaps = false; setSingleLine(true); ellipsize = TextUtils.TruncateAt.END
        minWidth = 0; minimumWidth = 0; minHeight = dp(context, 48); minimumHeight = dp(context, 48); includeFontPadding = false
        setPadding(dp(context, 12), 0, dp(context, 12), 0); gravity = Gravity.CENTER; stateListAnimator = null; elevation = 0f
        setOnClickListener { action.run() }
    }
    @JvmStatic fun buttonTheme(button: Button, colors: NativeReader.Appearance, primary: Boolean) {
        val ink = if (primary) colors.onAccent else colors.ink
        button.setTextColor(ColorStateList(arrayOf(intArrayOf(-android.R.attr.state_enabled), intArrayOf()), intArrayOf(colors.muted, ink)))
        button.backgroundTintList = null
        button.background = touch(button.context, if (primary) colors.accent else colors.panel, if (primary) colors.onAccent else colors.accent, 14)
    }
    @JvmStatic fun icon(button: Button, kind: Int, color: Int, iconOnly: Boolean) {
        button.compoundDrawablePadding = if (iconOnly) 0 else dp(button.context, 6)
        button.setCompoundDrawablesRelative(ShelfIcon(kind, color, dp(button.context, 20)), null, null, null)
        if (iconOnly) { button.text = ""; button.setPadding(dp(button.context, 14), 0, dp(button.context, 14), 0) }
    }
    @JvmStatic fun text(context: Context, value: String, size: Int, color: Int, medium: Boolean) = TextView(context).apply {
        text = value; textSize = size.toFloat(); setTextColor(color); typeface = if (medium) MEDIUM else REGULAR; includeFontPadding = false; gravity = Gravity.CENTER_VERTICAL
    }
    @JvmStatic fun singleLine(text: TextView) { text.setSingleLine(true); text.ellipsize = TextUtils.TruncateAt.END }
}
