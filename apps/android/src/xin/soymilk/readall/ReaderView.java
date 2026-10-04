package xin.soymilk.readall;

import android.content.Context;
import android.graphics.Bitmap;
import android.graphics.Canvas;
import android.graphics.Paint;
import android.graphics.RectF;
import android.view.GestureDetector;
import android.view.MotionEvent;
import android.view.View;

/** Displays only completed native frames; never parses a book from onDraw. */
final class ReaderView extends View {
    interface Listener { void viewport(int width,int height);void turn(boolean next); }
    private Bitmap bitmap;
    private final Paint paint=new Paint(Paint.FILTER_BITMAP_FLAG);
    private final GestureDetector gestures;
    private final Listener listener;
    ReaderView(Context context,Listener listener) {
        super(context);this.listener=listener;
        setContentDescription("阅读页面。左右滑动翻页，点击正文不翻页。");setFocusable(true);
        final float threshold=48*getResources().getDisplayMetrics().density;
        gestures=new GestureDetector(context,new GestureDetector.SimpleOnGestureListener() {
            @Override public boolean onDown(MotionEvent e) { return true; }
            @Override public boolean onSingleTapUp(MotionEvent e) { performClick();return true; }
            @Override public boolean onFling(MotionEvent start,MotionEvent end,float vx,float vy) {
                if(start==null) return false;
                float dx=end.getX()-start.getX(),dy=end.getY()-start.getY();
                if(Math.abs(dx)>=threshold && Math.abs(dx)>Math.abs(dy)*1.25f) { listener.turn(dx<0);return true; }
                return false;
            }
        });
    }
    void picture(Bitmap bitmap) { this.bitmap=bitmap;invalidate(); }
    @Override protected void onSizeChanged(int w,int h,int oldw,int oldh) {
        super.onSizeChanged(w,h,oldw,oldh);if(w>0 && h>0) listener.viewport(w,h);
    }
    @Override protected void onDraw(Canvas canvas) {
        super.onDraw(canvas);canvas.drawColor(0xfff5f6f8);
        if(bitmap!=null) {
            float scale=Math.min((float)getWidth()/bitmap.getWidth(),(float)getHeight()/bitmap.getHeight());
            float w=bitmap.getWidth()*scale,h=bitmap.getHeight()*scale;
            canvas.drawBitmap(bitmap,null,new RectF((getWidth()-w)/2,(getHeight()-h)/2,(getWidth()+w)/2,(getHeight()+h)/2),paint);
        }
    }
    @Override public boolean onTouchEvent(MotionEvent event) { return gestures.onTouchEvent(event); }
    @Override public boolean performClick() { super.performClick();return true; }
}
