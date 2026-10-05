package xin.soymilk.readall

/** UI-thread transient notice policy with an injected monotonic clock. */
class ShelfNotice {
    private var text = ""; private var working = false; private var deadline = -1L
    fun update(value: String?, busy: Boolean, now: Long, timeout: Int) { text = value.orEmpty(); working = busy; deadline = if (working || text.isEmpty()) -1 else now + maxOf(TIMEOUT_MS, timeout) }
    fun text() = text
    fun visible() = working || text.isNotEmpty()
    fun working() = working
    fun remaining(now: Long): Long = if (deadline < 0) -1 else maxOf(0, deadline - now)
    fun expire(now: Long) { if (!working && deadline >= 0 && now >= deadline) clear() }
    // Active work keeps its phase and Cancel button, even during lifecycle changes.
    fun dismiss() { if (!working) clear() }
    private fun clear() { text = ""; deadline = -1 }
    companion object { const val TIMEOUT_MS = 6000 }
}
