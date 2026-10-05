package xin.soymilk.readall;

/** UI-thread loading policy. Native work is not a reason to cover a readable page. */
final class LoadingFeedback {
    static final int NONE=0, INITIAL=1, CORNER=2;
    static final long SHOW_DELAY_MS=200, MIN_VISIBLE_MS=160;
    private long busySince=-1, shownSince=-1;

    int update(boolean hasDisplayedPage,boolean busy,long now){
        // A native serial is not enough: the first bitmap must reach ReaderView.
        // Keep the cancellable initial panel until then, even after native work finishes.
        if(!hasDisplayedPage){reset();return INITIAL;}
        if(!busy){
            busySince=-1;
            if(shownSince>=0&&now-shownSince<MIN_VISIBLE_MS)return CORNER;
            shownSince=-1;return NONE;
        }
        if(busySince<0||now<busySince)busySince=now;
        if(shownSince>=0)return CORNER;
        if(now-busySince<SHOW_DELAY_MS)return NONE;
        shownSince=now;return CORNER;
    }
    // New books, cancellation, errors and backgrounding must not inherit a spinner.
    void reset(){busySince=-1;shownSince=-1;}
}
