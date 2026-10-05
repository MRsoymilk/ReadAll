package xin.soymilk.readall

/** Native work is not a reason to cover an already displayed reading page. */
class LoadingFeedback {
    private var busySince = -1L; private var shownSince = -1L
    fun update(hasDisplayedPage: Boolean, busy: Boolean, now: Long): Int {
        if (!hasDisplayedPage) { reset(); return INITIAL }
        if (!busy) {
            busySince = -1
            if (shownSince >= 0 && now - shownSince < MIN_VISIBLE_MS) return CORNER
            shownSince = -1; return NONE
        }
        if (busySince < 0 || now < busySince) busySince = now
        if (shownSince >= 0) return CORNER
        if (now - busySince < SHOW_DELAY_MS) return NONE
        shownSince = now; return CORNER
    }
    fun reset() { busySince = -1; shownSince = -1 }
    companion object { const val NONE = 0; const val INITIAL = 1; const val CORNER = 2; const val SHOW_DELAY_MS = 200L; const val MIN_VISIBLE_MS = 160L }
}
