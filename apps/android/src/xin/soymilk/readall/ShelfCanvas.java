package xin.soymilk.readall;

import android.content.Context;
import android.graphics.*;
import android.view.*;
import android.view.accessibility.AccessibilityNodeInfo;
import android.os.Bundle;
import android.widget.OverScroller;
import java.util.*;

/** Virtualized shelf: draw only visible cards, or at most ten overlapping perspective covers. */
final class ShelfCanvas extends View {
    interface Listener {void open(ShelfStore.Book book);void menu(ShelfStore.Book book);void focused(String id);}
    private final float density;
    private final Paint paint=new Paint(Paint.ANTI_ALIAS_FLAG|Paint.FILTER_BITMAP_FLAG);
    private final Camera camera=new Camera();
    private final OverScroller scroller;
    private final GestureDetector gestures;
    private final Listener listener;
    private final ShelfCoverCache covers;
    private final ArrayList<Hit> hits=new ArrayList<>();
    private List<ShelfStore.Book> books=Collections.emptyList();
    private NativeReader.Appearance colors;
    private float offset,scrollUnit=1;
    private int mode=ShelfGeometry.COVERS;
    private boolean suppressTap,snapping;
    private String pendingFocus;
    private String focused="",emptyTitle="书架还没有图书",emptyNote="点击右上角「＋导入」，支持多选";
    private static final class Hit {
        final int index;final RectF bounds,menu;final Matrix inverse;
        Hit(int index,RectF bounds,RectF menu,Matrix inverse){this.index=index;this.bounds=bounds;this.menu=menu;this.inverse=inverse;}
        boolean contains(float x,float y){float[] p={x,y};if(inverse!=null)inverse.mapPoints(p);return bounds.contains(p[0],p[1]);}
    }
    ShelfCanvas(Context context,ShelfCoverCache covers,NativeReader.Appearance colors,Listener listener){
        super(context);this.covers=covers;this.colors=colors;this.listener=listener;density=getResources().getDisplayMetrics().density;
        scroller=new OverScroller(context);setFocusable(true);setContentDescription("书库");
        gestures=new GestureDetector(context,new GestureDetector.SimpleOnGestureListener(){
            @Override public boolean onDown(MotionEvent e){suppressTap=!scroller.isFinished();stop();return true;}
            @Override public boolean onScroll(MotionEvent a,MotionEvent b,float dx,float dy){offset=clamp(offset+(mode==ShelfGeometry.FLOW?dx/density/flowStep():dy/density));invalidate();return true;}
            @Override public boolean onFling(MotionEvent a,MotionEvent b,float vx,float vy){
                scrollUnit=density*(mode==ShelfGeometry.FLOW?flowStep():1);float speed=mode==ShelfGeometry.FLOW?-vx:-vy;
                scroller.fling(Math.round(offset*scrollUnit),0,Math.round(Math.max(-12000,Math.min(12000,speed))),0,0,Math.round(max()*scrollUnit),0,0);snapping=false;postInvalidateOnAnimation();return true;
            }
            @Override public boolean onSingleTapUp(MotionEvent e){
                if(suppressTap)return true;Hit h=hit(e.getX()/density,e.getY()/density);if(h==null)return true;
                if(mode==ShelfGeometry.FLOW&&Math.abs(h.index-offset)>.12f){center(h.index);return true;}
                if(h.menu!=null&&h.menu.contains(e.getX()/density,e.getY()/density))listener.menu(books.get(h.index));else listener.open(books.get(h.index));performClick();return true;
            }
            @Override public void onLongPress(MotionEvent e){Hit h=hit(e.getX()/density,e.getY()/density);if(h!=null){stop();listener.menu(books.get(h.index));}}
        });
    }
    float width(){return Math.max(1,getWidth()/density);}float height(){return Math.max(1,getHeight()/density);}
    private float max(){return ShelfGeometry.maxOffset(width(),height(),books.size(),mode);}
    private float clamp(float n){return ShelfGeometry.clamp(n,max());}
    private float flowWidth(){float footer=height()<240?60:124;return Math.max(24,Math.min(252,Math.min(width()*.57f,Math.max(24,(height()-footer)/1.40f))));}
    private float flowStep(){return flowWidth()*.53f;}
    void theme(NativeReader.Appearance value){colors=value;invalidate();}
    void empty(boolean filtering){emptyTitle=filtering?"没有找到匹配的图书":"书架还没有图书";emptyNote=filtering?"尝试其他书名、作者或格式":"点击右上角「＋导入」，支持多选";}
    void data(List<ShelfStore.Book> values,int requested,String anchor){
        String old=anchor==null?(pendingFocus==null?focusId():pendingFocus):anchor;float previousOffset=offset;int previousMode=mode;
        books=new ArrayList<>(values);mode=ShelfGeometry.mode(requested);stop();
        int at=find(old);offset=clamp(ShelfGeometry.restoreOffset(width(),mode,at,width(),previousMode,previousOffset));
        pendingFocus=getWidth()>0?null:old;hits.clear();invalidate();if(pendingFocus==null)notifyFocus();
    }
    private int find(String id){for(int i=0;i<books.size();i++)if(books.get(i).id.equals(id))return i;return -1;}
    String focusId(){if(pendingFocus!=null)return pendingFocus;int i=mode==ShelfGeometry.FLOW?ShelfGeometry.focused(books.size(),offset):ShelfGeometry.first(width(),books.size(),mode,offset);return i>=0&&i<books.size()?books.get(i).id:"";}
    void stop(){scroller.forceFinished(true);snapping=false;}
    private void center(int index){scrollUnit=density*(mode==ShelfGeometry.FLOW?flowStep():1);int start=Math.round(offset*scrollUnit),end=Math.round(clamp(index)*scrollUnit);scroller.startScroll(start,0,end-start,0,220);snapping=true;postInvalidateOnAnimation();}
    private void snap(){if(mode==ShelfGeometry.FLOW&&!books.isEmpty()&&Math.abs(offset-Math.round(offset))>.001f)center(Math.round(offset));else notifyFocus();}
    private void notifyFocus(){String id=focusId();if(!id.equals(focused)){focused=id;listener.focused(id);int i=ShelfGeometry.focused(books.size(),mode==ShelfGeometry.FLOW?offset:ShelfGeometry.first(width(),books.size(),mode,offset));setContentDescription(i>=0?books.get(i).label()+"，"+(i+1)+" / "+books.size()+"，点击阅读，长按管理":"书库为空");}}
    @Override protected void onSizeChanged(int w,int h,int oldw,int oldh){
        stop();int at=pendingFocus!=null?find(pendingFocus):mode==ShelfGeometry.FLOW?ShelfGeometry.focused(books.size(),offset):ShelfGeometry.first(Math.max(1,oldw/density),books.size(),mode,offset);
        offset=clamp(ShelfGeometry.restoreOffset(Math.max(1,w/density),mode,at,Math.max(1,oldw/density),mode,oldw>0?offset:0));pendingFocus=null;hits.clear();invalidate();notifyFocus();
    }
    @Override public boolean onTouchEvent(MotionEvent event){if(event.getPointerCount()>1){MotionEvent cancel=MotionEvent.obtain(event);cancel.setAction(MotionEvent.ACTION_CANCEL);gestures.onTouchEvent(cancel);cancel.recycle();stop();return true;}gestures.onTouchEvent(event);if(event.getActionMasked()==MotionEvent.ACTION_UP&&scroller.isFinished())snap();if(event.getActionMasked()==MotionEvent.ACTION_CANCEL){stop();snap();}return true;}
    @Override public boolean performClick(){super.performClick();return true;}
    @Override public void computeScroll(){if(scroller.computeScrollOffset()){offset=clamp(scroller.getCurrX()/scrollUnit);postInvalidateOnAnimation();if(scroller.isFinished()){if(snapping){snapping=false;notifyFocus();}else snap();}}}
    @Override protected void onDetachedFromWindow(){stop();super.onDetachedFromWindow();}
    @Override public boolean onGenericMotionEvent(MotionEvent e){if(e.getAction()==MotionEvent.ACTION_SCROLL){stop();offset=clamp(offset-e.getAxisValue(MotionEvent.AXIS_VSCROLL)*(mode==ShelfGeometry.FLOW?1:52));invalidate();snap();return true;}return super.onGenericMotionEvent(e);}
    @Override public boolean onKeyDown(int key,KeyEvent event){
        if(key==KeyEvent.KEYCODE_DPAD_LEFT||key==KeyEvent.KEYCODE_DPAD_UP){move(-1);return true;}
        if(key==KeyEvent.KEYCODE_DPAD_RIGHT||key==KeyEvent.KEYCODE_DPAD_DOWN){move(1);return true;}
        if(key==KeyEvent.KEYCODE_ENTER||key==KeyEvent.KEYCODE_DPAD_CENTER){openFocused(false);return true;}return super.onKeyDown(key,event);
    }
    private void move(int direction){stop();if(mode==ShelfGeometry.FLOW)center(Math.round(offset)+direction);else{offset=clamp(offset+direction*height()*.8f);invalidate();notifyFocus();}}
    private void openFocused(boolean menu){String id=focusId();for(ShelfStore.Book b:books)if(b.id.equals(id)){if(menu)listener.menu(b);else listener.open(b);break;}}
    @Override public void onInitializeAccessibilityNodeInfo(AccessibilityNodeInfo info){super.onInitializeAccessibilityNodeInfo(info);info.setScrollable(true);info.addAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_SCROLL_FORWARD);info.addAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_SCROLL_BACKWARD);info.addAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_CLICK);info.addAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_LONG_CLICK);}
    @Override public boolean performAccessibilityAction(int action,Bundle args){if(action==AccessibilityNodeInfo.ACTION_SCROLL_FORWARD){move(1);return true;}if(action==AccessibilityNodeInfo.ACTION_SCROLL_BACKWARD){move(-1);return true;}if(action==AccessibilityNodeInfo.ACTION_CLICK){openFocused(false);return true;}if(action==AccessibilityNodeInfo.ACTION_LONG_CLICK){openFocused(true);return true;}return super.performAccessibilityAction(action,args);}
    private Hit hit(float x,float y){for(int i=hits.size()-1;i>=0;i--)if(hits.get(i).contains(x,y))return hits.get(i);return null;}
    @Override protected void onDraw(Canvas canvas){
        super.onDraw(canvas);canvas.save();canvas.scale(density,density);canvas.drawColor(colors.canvas);hits.clear();
        if(books.isEmpty()){label(canvas,emptyTitle,24,Math.max(32,height()*.34f),width()-48,23,colors.ink);label(canvas,emptyNote,24,Math.max(63,height()*.34f+36),width()-48,14,colors.muted);canvas.restore();return;}
        if(mode==ShelfGeometry.FLOW)flow(canvas);else flat(canvas);canvas.restore();
    }
    private void flat(Canvas c){
        float w=width(),pitch=ShelfGeometry.pitch(w,mode),cw=ShelfGeometry.cellWidth(w);int cols=mode==ShelfGeometry.LIST?1:ShelfGeometry.columns(w);
        int first=ShelfGeometry.first(w,books.size(),mode,offset),end=ShelfGeometry.end(w,height(),books.size(),mode,offset);
        for(int i=first;i<end;i++){
            ShelfStore.Book b=books.get(i);float x=mode==ShelfGeometry.LIST?16:16+(i%cols)*(cw+14),y=10+(i/cols)*pitch-offset;
            RectF rect=new RectF(x,y,mode==ShelfGeometry.LIST?w-16:x+cw,y+pitch-12);round(c,rect,12,colors.panel);
            RectF menu=new RectF(rect.right-34,rect.top+3,rect.right,rect.top+33);
            if(mode==ShelfGeometry.LIST){
                cover(c,b,new RectF(x+10,y+9,x+68,y+87));float tx=x+82;label(c,b.label(),tx,y+28,rect.right-tx-36,16,colors.ink);label(c,b.author.isEmpty()?b.name:b.author,tx,y+49,rect.right-tx-16,12,colors.muted);label(c,(b.pinned?"置顶 · ":"")+b.format+" · "+b.progress(),tx,y+69,rect.right-tx-16,11,colors.muted);bar(c,tx,y+79,rect.right-tx-16,b.percent);
            }else{
                cover(c,b,new RectF(x,y,x+cw,y+cw*1.40f));float baseline=y+cw*1.40f;label(c,b.label(),x+8,baseline+21,cw-16,14,colors.ink);label(c,(b.pinned?"置顶 · ":"")+b.format+" · "+b.progress(),x+8,baseline+42,cw-16,11,colors.muted);bar(c,x+8,baseline+51,cw-16,b.percent);
                round(c,menu,10,colors.panel);
            }
            label(c,"⋯",menu.left+7,menu.top+23,26,22,colors.ink);hits.add(new Hit(i,rect,menu,null));
        }
        float maximum=max();if(maximum>0){float sh=Math.max(26,height()*height()/(maximum+height())),sy=(height()-sh)*offset/maximum;round(c,new RectF(w-4,sy,w-2,sy+sh),1,colors.border);}
    }
    private void flow(Canvas c){
        float w=width(),h=height();if(h<74){centered(c,"收起键盘或转为竖屏查看立体书架",h*.6f,w-24,12,colors.muted);return;}float bw=flowWidth(),bh=bw*1.40f,cy=(h-(h<240?48:110))*.5f,cx=w/2;
        ArrayList<Integer> order=new ArrayList<>();int first=ShelfGeometry.first(w,books.size(),mode,offset),end=ShelfGeometry.end(w,h,books.size(),mode,offset);
        for(int i=first;i<end;i++)order.add(i);order.sort((a,b)->Float.compare(Math.abs(b-offset),Math.abs(a-offset)));
        for(int i:order){
            ShelfGeometry.Pose pose=new ShelfGeometry.Pose(i-offset,bw);Matrix matrix=new Matrix();camera.save();camera.setLocation(0,0,-8);camera.rotateY(pose.angle);camera.getMatrix(matrix);camera.restore();
            matrix.preTranslate(-bw/2,-bh/2);matrix.postScale(pose.scale,pose.scale);matrix.postTranslate(cx+pose.x,cy+Math.abs(i-offset)*7);
            c.save();c.concat(matrix);RectF rect=new RectF(0,0,bw,bh);round(c,new RectF(4,8,bw+7,bh+8),7,(colors.ink&0x00ffffff)|0x18000000);cover(c,books.get(i),rect);
            paint.setColor((colors.canvas&0x00ffffff)|((int)((1-pose.alpha)*155)<<24));c.drawRect(rect,paint);c.restore();
            Matrix inverse=new Matrix();if(matrix.invert(inverse))hits.add(new Hit(i,rect,null,inverse));
        }
        int at=ShelfGeometry.focused(books.size(),offset);ShelfStore.Book b=books.get(at);
        if(h<240){centered(c,b.label(),h-25,w-36,14,colors.ink);centered(c,(at+1)+" / "+books.size()+" · 左右滑动 · 长按管理",h-8,w-28,10,colors.muted);}
        else{float y=Math.min(h-88,cy+bh/2+29);centered(c,b.label(),y,w-36,20,colors.ink);centered(c,(b.author.isEmpty()?b.format:b.author+" · "+b.format)+" · "+b.progress(),y+26,w-36,12,colors.muted);bar(c,w*.3f,y+40,w*.4f,b.percent);centered(c,(at+1)+" / "+books.size()+"    左右滑动 · 点封面阅读 · 长按管理",h-16,w-28,11,colors.muted);}
    }
    private void cover(Canvas c,ShelfStore.Book book,RectF r){
        Bitmap image=covers.get(book.id);c.save();c.clipRect(r);round(c,r,5,colors.button);
        if(image!=null){paint.setColor(Color.WHITE);c.drawRect(r,paint);float scale=Math.min(r.width()/image.getWidth(),r.height()/image.getHeight());float ww=image.getWidth()*scale,hh=image.getHeight()*scale;RectF fit=new RectF(r.centerX()-ww/2,r.centerY()-hh/2,r.centerX()+ww/2,r.centerY()+hh/2);paint.setColor(Color.WHITE);c.drawBitmap(image,null,fit,paint);}
        else {
            paint.setColor(colors.accent);c.drawRect(r.left,r.top,r.left+Math.max(3,r.width()*.045f),r.bottom,paint);
            float pad=r.width()*.12f;float size=Math.min(23,r.width()*.13f);String text=book.label();
            label(c,book.format,r.left+pad,r.top+r.height()*.17f,r.width()-pad*2,Math.max(6,size*.55f),colors.muted);
            for(int line=0;line<3&&!text.isEmpty();line++){
                paint.setTypeface(Typeface.create("sans-serif-medium",Typeface.NORMAL));paint.setTextSize(size);int n=paint.breakText(text,true,r.width()-pad*2,null);if(n>0&&n<text.length()&&Character.isHighSurrogate(text.charAt(n-1)))n--;if(n<=0)break;
                label(c,text.substring(0,n),r.left+pad,r.top+r.height()*.41f+line*size*1.4f,r.width()-pad*2,size,colors.ink);text=text.substring(n);
            }
            label(c,"READALL",r.left+pad,r.bottom-r.height()*.10f,r.width()-pad*2,Math.max(6,size*.48f),colors.muted);
        }
        paint.setColor((colors.border&0x00ffffff)|0x85000000);paint.setStyle(Paint.Style.STROKE);paint.setStrokeWidth(.6f);c.drawRect(r,paint);paint.setStyle(Paint.Style.FILL);c.restore();
    }
    private void round(Canvas c,RectF r,float radius,int color){paint.setColor(color);paint.setStyle(Paint.Style.FILL);c.drawRoundRect(r,radius,radius,paint);}
    private void bar(Canvas c,float x,float y,float w,double progress){round(c,new RectF(x,y,x+w,y+2),1,colors.border);if(progress>=0)round(c,new RectF(x,y,x+w*(float)(progress/100),y+2),1,colors.accent);}
    private String fit(String text,float width){if(paint.measureText(text)<=width)return text;int n=paint.breakText(text,true,Math.max(0,width-paint.measureText("…")),null);if(n>0&&n<text.length()&&Character.isHighSurrogate(text.charAt(n-1)))n--;return text.substring(0,n)+"…";}
    private void label(Canvas c,String text,float x,float y,float width,float size,int color){paint.setTypeface(Typeface.create("sans-serif",Typeface.NORMAL));paint.setTextSize(size);paint.setColor(color);paint.setStyle(Paint.Style.FILL);c.drawText(fit(text.replace('\n',' '),Math.max(0,width)),x,y,paint);}
    private void centered(Canvas c,String text,float y,float width,float size,int color){paint.setTypeface(Typeface.create("sans-serif",Typeface.NORMAL));paint.setTextSize(size);String t=fit(text,width);label(c,t,(this.width()-paint.measureText(t))/2,y,width,size,color);}
}
