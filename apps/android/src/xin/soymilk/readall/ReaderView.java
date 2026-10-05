package xin.soymilk.readall;

import android.content.Context;
import android.graphics.Bitmap;
import android.graphics.Canvas;
import android.graphics.Paint;
import android.graphics.RectF;
import android.view.KeyEvent;
import android.view.MotionEvent;
import android.view.VelocityTracker;
import android.view.View;
import android.view.ViewConfiguration;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.view.inputmethod.InputMethodManager;

/** The whole reading surface, including Linux's chrome and overlays, comes from Rust. */
final class ReaderView extends View {
    interface Listener { void viewport(int width,int height);void action(int code,int a,int b);void input(String mode,String text); }
    private Bitmap bitmap;
    private int layoutWidth,layoutHeight,backgroundColor;
    private final Paint paint=new Paint(Paint.FILTER_BITMAP_FLAG);
    private final Listener listener;
    private final TouchRouter touch;
    private VelocityTracker velocity;
    private NativeReader.State state;
    private ReaderInput connection;
    private boolean ready;
    private String editorMode="";
    private final Runnable longPress;
    ReaderView(Context context,Listener listener) {
        super(context);this.listener=listener;setFocusable(true);setFocusableInTouchMode(true);
        // onDraw consumes the bitmap synchronously on the UI thread. The retired
        // offscreen bitmap can then be reused without a RenderThread alias/race.
        setLayerType(View.LAYER_TYPE_SOFTWARE,null);
        setContentDescription("ReadAll 阅读页面。滑动翻页，长按拖选文字；底部箭头展开工具栏和目录。");
        touch=new TouchRouter(8,(kind,x,y)->listener.action(NativeReader.TOUCH+kind,x,y));
        longPress=()-> { if(ready)touch.longPress(); };
    }
    void background(int color){backgroundColor=color;invalidate();}
    int[] viewportSize() {
        return ReaderViewport.sizes(Math.max(1,getWidth()),Math.max(1,getHeight()),getResources().getDisplayMetrics().density);
    }
    Bitmap picture(Bitmap image) { return picture(image,null); }
    Bitmap picture(Bitmap image,NativeReader.State frame) {
        Bitmap old=bitmap;bitmap=image;
        if(frame!=null){layoutWidth=frame.logicalWidth;layoutHeight=frame.logicalHeight;}
        ready=image!=null&&layoutWidth>0&&layoutHeight>0;invalidate();return old;
    }
    boolean touching() { return touch.active(); }
    void state(NativeReader.State next) {
        state=next;
        if(bitmap!=null) {
            boolean matches=next.logicalWidth==layoutWidth&&next.logicalHeight==layoutHeight&&next.width==bitmap.getWidth()&&next.height==bitmap.getHeight();
            if(!matches&&touch.active())cancelTouch();
            ready=matches;
        }
        String mode=next.editing?next.uiMode:"";
        if(!mode.equals(editorMode)) {
            editorMode=mode;connection=null;
            InputMethodManager ime=(InputMethodManager)getContext().getSystemService(Context.INPUT_METHOD_SERVICE);
            ime.restartInput(this);
            if(!mode.isEmpty()) { requestFocus();post(()->ime.showSoftInput(this,InputMethodManager.SHOW_IMPLICIT)); }
            else ime.hideSoftInputFromWindow(getWindowToken(),0);
        }
        if(connection!=null)connection.nativeText(next.input);
    }
    void cancelTouch() { removeCallbacks(longPress);touch.cancel();if(velocity!=null){velocity.recycle();velocity=null;} }
    void closeInput() { editorMode="";connection=null;((InputMethodManager)getContext().getSystemService(Context.INPUT_METHOD_SERVICE)).hideSoftInputFromWindow(getWindowToken(),0); }
    private float scale() { return bitmap==null?1:Math.min((float)getWidth()/bitmap.getWidth(),(float)getHeight()/bitmap.getHeight()); }
    private int[] point(float x,float y) {
        return ReaderViewport.point(x,y,getWidth(),getHeight(),bitmap.getWidth(),bitmap.getHeight(),layoutWidth,layoutHeight);
    }
    @Override protected void onSizeChanged(int w,int h,int oldw,int oldh) { super.onSizeChanged(w,h,oldw,oldh);cancelTouch();if(w>0&&h>0)listener.viewport(w,h); }
    @Override protected void onDraw(Canvas canvas) {
        super.onDraw(canvas);canvas.drawColor(backgroundColor);
        if(bitmap!=null) { float s=scale(),w=bitmap.getWidth()*s,h=bitmap.getHeight()*s;paint.setFilterBitmap(bitmap.getWidth()!=getWidth()||bitmap.getHeight()!=getHeight());canvas.drawBitmap(bitmap,null,new RectF((getWidth()-w)/2,(getHeight()-h)/2,(getWidth()+w)/2,(getHeight()+h)/2),paint); }
    }
    @Override public boolean onTouchEvent(MotionEvent event) {
        if(!ready)return true;
        if(event.getPointerCount()>1 || event.getActionMasked()==MotionEvent.ACTION_CANCEL) { cancelTouch();return true; }
        int[] p=point(event.getX(),event.getY());
        switch(event.getActionMasked()) {
            case MotionEvent.ACTION_DOWN:
                requestFocus();cancelTouch();velocity=VelocityTracker.obtain();velocity.addMovement(event);touch.down(p[0],p[1]);postDelayed(longPress,ViewConfiguration.getLongPressTimeout());return true;
            case MotionEvent.ACTION_MOVE:
                if(velocity!=null)velocity.addMovement(event);touch.move(p[0],p[1]);return true;
            case MotionEvent.ACTION_UP:
                removeCallbacks(longPress);int fling=0;
                if(velocity!=null){velocity.addMovement(event);velocity.computeCurrentVelocity(1000);if(state!=null&&"scroll".equals(state.pageMode))fling=Math.round(-velocity.getYVelocity()/scale()*layoutHeight/bitmap.getHeight()*0.15f);velocity.recycle();velocity=null;}
                touch.up(p[0],p[1],fling);performClick();
                if(!editorMode.isEmpty()&&p[1]>=78&&p[1]<122)((InputMethodManager)getContext().getSystemService(Context.INPUT_METHOD_SERVICE)).showSoftInput(this,InputMethodManager.SHOW_IMPLICIT);
                return true;
            default:return true;
        }
    }
    @Override public boolean performClick(){super.performClick();return true;}
    @Override public boolean onCheckIsTextEditor(){return !editorMode.isEmpty();}
    @Override public InputConnection onCreateInputConnection(EditorInfo out){
        if(editorMode.isEmpty()||state==null)return null;
        out.inputType=android.text.InputType.TYPE_CLASS_TEXT | ("note".equals(editorMode)?android.text.InputType.TYPE_TEXT_FLAG_MULTI_LINE:0);
        out.imeOptions=("search".equals(editorMode)?EditorInfo.IME_ACTION_SEARCH:EditorInfo.IME_ACTION_DONE)|EditorInfo.IME_FLAG_NO_EXTRACT_UI;
        connection=new ReaderInput(this,editorMode,state.input,(mode,text)->listener.input(mode,text),()->{listener.action(NativeReader.ACTIVATE,0,0);((InputMethodManager)getContext().getSystemService(Context.INPUT_METHOD_SERVICE)).hideSoftInputFromWindow(getWindowToken(),0);});
        return connection;
    }
    @Override public boolean onKeyDown(int key,KeyEvent event){
        int code=0;
        if(event.isCtrlPressed()){if(key==KeyEvent.KEYCODE_C)code=NativeReader.COPY;if(key==KeyEvent.KEYCODE_V)code=NativeReader.PASTE;}
        else switch(key){
            case KeyEvent.KEYCODE_F2:code=NativeReader.FIND;break;case KeyEvent.KEYCODE_F3:code=NativeReader.ANNOTATIONS;break;case KeyEvent.KEYCODE_F4:code=NativeReader.BOOKMARK;break;
            case KeyEvent.KEYCODE_F5:code=NativeReader.SETTINGS;break;case KeyEvent.KEYCODE_F6:code=NativeReader.THEME;break;case KeyEvent.KEYCODE_F7:code=NativeReader.NOTE;break;case KeyEvent.KEYCODE_F8:code=NativeReader.HIGHLIGHT;break;
            case KeyEvent.KEYCODE_PAGE_DOWN:case KeyEvent.KEYCODE_DPAD_DOWN:case KeyEvent.KEYCODE_DPAD_RIGHT:code=NativeReader.NEXT;break;
            case KeyEvent.KEYCODE_PAGE_UP:case KeyEvent.KEYCODE_DPAD_UP:case KeyEvent.KEYCODE_DPAD_LEFT:code=NativeReader.PREVIOUS;break;
            case KeyEvent.KEYCODE_ESCAPE:code=NativeReader.BACK;break;case KeyEvent.KEYCODE_ENTER:code=NativeReader.ACTIVATE;break;
            default:break;
        }
        if(code!=0){listener.action(code,0,0);return true;}return super.onKeyDown(key,event);
    }
}
