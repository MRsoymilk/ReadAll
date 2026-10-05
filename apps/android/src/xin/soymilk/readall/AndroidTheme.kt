@file:Suppress("DEPRECATION") // Version-gated legacy system bars remain supported on API 26–29.
package xin.soymilk.readall

import android.app.Activity
import android.content.res.ColorStateList
import android.graphics.drawable.ColorDrawable
import android.os.Build
import android.view.View
import android.view.ViewGroup
import android.view.WindowInsetsController
import android.widget.Button
import android.widget.ProgressBar
import android.widget.TextView

/** Platform surfaces consume the shared Rust palette, without a second RGB palette. */
object AndroidTheme {
    @JvmStatic fun apply(activity: Activity, root: ViewGroup, home: View, loading: View, page: ReaderView, progress: ProgressBar, pageLoading: PageLoadingIndicator, colors: NativeReader.Appearance) {
        root.setBackgroundColor(colors.canvas); home.setBackgroundColor(colors.canvas); loading.background = ShelfStyle.shape(activity, colors.panel, 20); page.background(colors.page)
        if (Build.VERSION.SDK_INT >= 29) root.isForceDarkAllowed = false
        tint(root, colors)
        progress.progressTintList = ColorStateList.valueOf(colors.accent); progress.progressBackgroundTintList = ColorStateList.valueOf(colors.border)
        progress.indeterminateTintList = ColorStateList.valueOf(colors.accent); pageLoading.indeterminateTintList = ColorStateList.valueOf(colors.accent)
        val window = activity.window; window.setBackgroundDrawable(ColorDrawable(colors.canvas)); window.statusBarColor = colors.canvas; window.navigationBarColor = colors.canvas
        if (Build.VERSION.SDK_INT >= 29) { window.isStatusBarContrastEnforced = false; window.isNavigationBarContrastEnforced = false }
        if (Build.VERSION.SDK_INT >= 30) {
            val mask = WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS or WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS
            window.insetsController?.setSystemBarsAppearance(if (colors.dark()) 0 else mask, mask)
        } else {
            val mask = View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR or View.SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR
            window.decorView.systemUiVisibility = (window.decorView.systemUiVisibility and mask.inv()) or if (colors.dark()) 0 else mask
        }
    }
    private fun tint(view: View, colors: NativeReader.Appearance) {
        if (view is ShelfHome) return // Its hierarchy owns selected tabs and icon colors.
        if (view is Button) { view.typeface = ShelfStyle.MEDIUM; view.stateListAnimator = null; view.elevation = 0f; ShelfStyle.buttonTheme(view, colors, false) }
        else if (view is TextView) view.setTextColor(colors.ink)
        if (view is ViewGroup) repeat(view.childCount) { tint(view.getChildAt(it), colors) }
    }
}
