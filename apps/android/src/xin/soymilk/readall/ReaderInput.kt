package xin.soymilk.readall

import android.text.Editable
import android.text.Selection
import android.text.SpannableStringBuilder
import android.view.KeyEvent
import android.view.View
import android.view.inputmethod.BaseInputConnection

/** Real Android composing buffer over the shared Rust search/note input. */
class ReaderInput(view: View, private val mode: String, initial: String, private val changed: Changed, private val submit: Runnable) : BaseInputConnection(view, true) {
    fun interface Changed { fun set(mode: String, text: String) }
    private val text: Editable = SpannableStringBuilder(initial).also { Selection.setSelection(it, it.length) }
    private var pending: String? = null
    override fun getEditable(): Editable = text
    private fun update() {
        val limit = if (mode == "search") 1024 else 8192
        while (text.toString().toByteArray(Charsets.UTF_8).size > limit && text.isNotEmpty()) { val end = text.length; val start = Character.offsetByCodePoints(text, end, -1); text.delete(start, end) }
        val value = text.toString(); pending = value; changed.set(mode, value)
    }
    fun nativeText(value: String) {
        if (pending != null) { if (value == pending) pending = null; return }
        if (text.toString() != value) { text.replace(0, text.length, value); Selection.setSelection(text, text.length) }
    }
    override fun commitText(value: CharSequence?, cursor: Int): Boolean { val ok = super.commitText(value, cursor); update(); return ok }
    override fun setComposingText(value: CharSequence?, cursor: Int): Boolean { val ok = super.setComposingText(value, cursor); update(); return ok }
    override fun finishComposingText(): Boolean { val ok = super.finishComposingText(); update(); return ok }
    override fun deleteSurroundingText(before: Int, after: Int): Boolean { val ok = super.deleteSurroundingText(before, after); update(); return ok }
    override fun deleteSurroundingTextInCodePoints(before: Int, after: Int): Boolean { val ok = super.deleteSurroundingTextInCodePoints(before, after); update(); return ok }
    override fun performEditorAction(action: Int): Boolean { finishComposingText(); submit.run(); return true }
    override fun sendKeyEvent(event: KeyEvent): Boolean {
        if (event.action != KeyEvent.ACTION_DOWN) return true
        if (event.keyCode == KeyEvent.KEYCODE_DEL) return deleteSurroundingTextInCodePoints(1, 0)
        if (event.keyCode == KeyEvent.KEYCODE_ENTER) { submit.run(); return true }
        val codePoint = event.unicodeChar
        return if (codePoint != 0) commitText(String(Character.toChars(codePoint)), 1) else super.sendKeyEvent(event)
    }
}
