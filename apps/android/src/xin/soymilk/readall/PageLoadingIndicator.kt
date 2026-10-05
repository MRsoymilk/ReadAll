package xin.soymilk.readall

import android.content.Context
import android.view.MotionEvent
import android.widget.ProgressBar

/** Independently animated corner spinner; it never consumes input or resizes the page. */
class PageLoadingIndicator(context: Context) : ProgressBar(context, null, android.R.attr.progressBarStyleSmall) {
    init { isIndeterminate = true; isClickable = false; isLongClickable = false; isFocusable = false; importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO; visibility = INVISIBLE }
    fun show(visible: Boolean) { val next = if (visible) VISIBLE else INVISIBLE; if (visibility != next) visibility = next }
    override fun dispatchTouchEvent(event: MotionEvent): Boolean = false
    override fun dispatchGenericMotionEvent(event: MotionEvent): Boolean = false
}
