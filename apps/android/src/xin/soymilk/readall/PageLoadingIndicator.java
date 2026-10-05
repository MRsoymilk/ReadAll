package xin.soymilk.readall;

import android.content.Context;
import android.view.MotionEvent;
import android.widget.ProgressBar;

/** A native, independently animated corner spinner, never an input overlay. */
final class PageLoadingIndicator extends ProgressBar {
    PageLoadingIndicator(Context context){
        super(context,null,android.R.attr.progressBarStyleSmall);
        setIndeterminate(true);setClickable(false);setLongClickable(false);setFocusable(false);
        setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO);
        setVisibility(INVISIBLE);
    }
    void show(boolean visible){
        int next=visible?VISIBLE:INVISIBLE;
        if(getVisibility()!=next)setVisibility(next);
    }
    // Return false even if a framework style changes the view's clickable state.
    // Gestures on this small visual can still start on the ReaderView underneath.
    @Override public boolean dispatchTouchEvent(MotionEvent event){return false;}
    @Override public boolean dispatchGenericMotionEvent(MotionEvent event){return false;}
}
