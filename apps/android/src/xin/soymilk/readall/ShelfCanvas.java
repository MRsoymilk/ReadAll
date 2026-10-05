package xin.soymilk.readall;

import android.content.Context;
import android.graphics.*;
import android.view.*;
import android.view.accessibility.AccessibilityNodeInfo;
import android.os.Bundle;
import android.widget.OverScroller;
import java.util.*;

/** Virtualized list/grid shelf: only visible cards are drawn; both modes scroll vertically. */
final class ShelfCanvas extends View {
    interface Listener {void open(ShelfStore.Book book);void menu(ShelfStore.Book book);void focused(String id);}
    private final float density;
    private final Paint paint=new Paint(Paint.ANTI_ALIAS_FLAG|Paint.FILTER_BITMAP_FLAG);
    private final Path coverClip=new Path();
    private ShelfIcon moreIcon,bookIcon,pinIcon;
    private int pressed=-1;
    private final OverScroller scroller;
    private final GestureDetector gestures;
    private final Listener listener;
    private final ShelfCoverCache covers;
    private final ArrayList<Hit> hits=new ArrayList<>();
    private List<ShelfStore.Book> books=Collections.emptyList();
    private NativeReader.Appearance colors;
    private float offset;
    private int mode=ShelfGeometry.COVERS;
    private boolean suppressTap;
    private String pendingFocus;
    private String focused="",emptyTitle="书架还没有图书",emptyNote="点击右上角「导入」，添加你的第一本书";
    private static final class Hit {
        final int index;final RectF bounds,menu;
        Hit(int index,RectF bounds,RectF menu){this.index=index;this.bounds=bounds;this.menu=menu;}
        boolean contains(float x,float y){return bounds.contains(x,y);}
    }
    ShelfCanvas(Context context,ShelfCoverCache covers,NativeReader.Appearance colors,Listener listener){
        super(context);this.covers=covers;this.colors=colors;this.listener=listener;density=getResources().getDisplayMetrics().density;theme(colors);
        scroller=new OverScroller(context);setFocusable(true);setContentDescription("书库");
        gestures=new GestureDetector(context,new GestureDetector.SimpleOnGestureListener(){
            @Override public boolean onDown(MotionEvent e){suppressTap=!scroller.isFinished();stop();Hit h=hit(e.getX()/density,e.getY()/density);pressed=suppressTap||h==null?-1:h.index;invalidate();return true;}
            @Override public boolean onScroll(MotionEvent a,MotionEvent b,float dx,float dy){pressed=-1;offset=clamp(offset+dy/density);postInvalidateOnAnimation();return true;}
            @Override public boolean onFling(MotionEvent a,MotionEvent b,float vx,float vy){
                int speed=Math.round(Math.max(-12000,Math.min(12000,-vy)));
                scroller.fling(0,Math.round(offset*density),0,speed,0,0,0,Math.round(max()*density));postInvalidateOnAnimation();return true;
            }
            @Override public boolean onSingleTapUp(MotionEvent e){
                if(suppressTap)return true;Hit h=hit(e.getX()/density,e.getY()/density);if(h==null)return true;
                if(h.menu!=null&&h.menu.contains(e.getX()/density,e.getY()/density))listener.menu(books.get(h.index));else listener.open(books.get(h.index));performClick();return true;
            }
            @Override public void onLongPress(MotionEvent e){Hit h=hit(e.getX()/density,e.getY()/density);if(h!=null){stop();listener.menu(books.get(h.index));}}
        });
    }
    float width(){return Math.max(1,getWidth()/density);}float height(){return Math.max(1,getHeight()/density);}
    private float max(){return ShelfGeometry.maxOffset(width(),height(),books.size(),mode);}
    private float clamp(float n){return ShelfGeometry.clamp(n,max());}
    void theme(NativeReader.Appearance value){colors=value;moreIcon=new ShelfIcon(ShelfIcon.MORE,value.ink,20);bookIcon=new ShelfIcon(ShelfIcon.BOOK,value.accent,28);pinIcon=new ShelfIcon(ShelfIcon.PIN,value.accent,14);invalidate();}
    void empty(boolean filtering){emptyTitle=filtering?"没有找到匹配的图书":"书架还没有图书";emptyNote=filtering?"尝试其他书名、作者或格式":"点击右上角「导入」，添加你的第一本书";}
    void data(List<ShelfStore.Book> values,int requested,String anchor){
        String old=anchor==null?(pendingFocus==null?focusId():pendingFocus):anchor;float previousOffset=offset;int previousMode=mode;
        books=new ArrayList<>(values);mode=ShelfGeometry.mode(requested);stop();
        int at=find(old);offset=clamp(ShelfGeometry.restoreOffset(width(),mode,at,width(),previousMode,previousOffset));
        pendingFocus=getWidth()>0?null:old;hits.clear();invalidate();if(pendingFocus==null)notifyFocus();
    }
    private int find(String id){for(int i=0;i<books.size();i++)if(books.get(i).id.equals(id))return i;return -1;}
    String focusId(){if(pendingFocus!=null)return pendingFocus;int i=ShelfGeometry.first(width(),books.size(),mode,offset);return i>=0&&i<books.size()?books.get(i).id:"";}
    void stop(){scroller.forceFinished(true);if(pressed!=-1){pressed=-1;invalidate();}}
    private void notifyFocus(){String id=focusId();if(!id.equals(focused)){focused=id;listener.focused(id);int i=ShelfGeometry.first(width(),books.size(),mode,offset);setContentDescription(i>=0&&i<books.size()?books.get(i).label()+"，"+(i+1)+" / "+books.size()+"，点击阅读，长按管理":"书库为空");}}
    @Override protected void onSizeChanged(int w,int h,int oldw,int oldh){
        stop();int at=pendingFocus!=null?find(pendingFocus):ShelfGeometry.first(Math.max(1,oldw/density),books.size(),mode,offset);
        offset=clamp(ShelfGeometry.restoreOffset(Math.max(1,w/density),mode,at,Math.max(1,oldw/density),mode,oldw>0?offset:0));pendingFocus=null;hits.clear();invalidate();notifyFocus();
    }
    @Override public boolean onTouchEvent(MotionEvent event){if(event.getPointerCount()>1){MotionEvent cancel=MotionEvent.obtain(event);cancel.setAction(MotionEvent.ACTION_CANCEL);gestures.onTouchEvent(cancel);cancel.recycle();stop();return true;}gestures.onTouchEvent(event);if(event.getActionMasked()==MotionEvent.ACTION_UP){pressed=-1;invalidate();if(scroller.isFinished())notifyFocus();}if(event.getActionMasked()==MotionEvent.ACTION_CANCEL){stop();notifyFocus();}return true;}
    @Override public boolean performClick(){super.performClick();return true;}
    @Override public void computeScroll(){if(scroller.computeScrollOffset()){offset=clamp(scroller.getCurrY()/density);postInvalidateOnAnimation();if(scroller.isFinished())notifyFocus();}}
    @Override protected void onDetachedFromWindow(){stop();super.onDetachedFromWindow();}
    @Override public boolean onGenericMotionEvent(MotionEvent e){if(e.getAction()==MotionEvent.ACTION_SCROLL){stop();offset=clamp(offset-e.getAxisValue(MotionEvent.AXIS_VSCROLL)*52);invalidate();notifyFocus();return true;}return super.onGenericMotionEvent(e);}
    @Override public boolean onKeyDown(int key,KeyEvent event){
        if(key==KeyEvent.KEYCODE_DPAD_LEFT||key==KeyEvent.KEYCODE_DPAD_UP){move(-1);return true;}
        if(key==KeyEvent.KEYCODE_DPAD_RIGHT||key==KeyEvent.KEYCODE_DPAD_DOWN){move(1);return true;}
        if(key==KeyEvent.KEYCODE_ENTER||key==KeyEvent.KEYCODE_DPAD_CENTER){openFocused(false);return true;}return super.onKeyDown(key,event);
    }
    private void move(int direction){stop();offset=clamp(offset+direction*height()*.8f);invalidate();notifyFocus();}
    private void openFocused(boolean menu){String id=focusId();for(ShelfStore.Book b:books)if(b.id.equals(id)){if(menu)listener.menu(b);else listener.open(b);break;}}
    @Override public void onInitializeAccessibilityNodeInfo(AccessibilityNodeInfo info){super.onInitializeAccessibilityNodeInfo(info);info.setScrollable(true);info.addAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_SCROLL_FORWARD);info.addAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_SCROLL_BACKWARD);info.addAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_CLICK);info.addAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_LONG_CLICK);}
    @Override public boolean performAccessibilityAction(int action,Bundle args){if(action==AccessibilityNodeInfo.ACTION_SCROLL_FORWARD){move(1);return true;}if(action==AccessibilityNodeInfo.ACTION_SCROLL_BACKWARD){move(-1);return true;}if(action==AccessibilityNodeInfo.ACTION_CLICK){openFocused(false);return true;}if(action==AccessibilityNodeInfo.ACTION_LONG_CLICK){openFocused(true);return true;}return super.performAccessibilityAction(action,args);}
    private Hit hit(float x,float y){for(int i=hits.size()-1;i>=0;i--)if(hits.get(i).contains(x,y))return hits.get(i);return null;}
    @Override protected void onDraw(Canvas canvas){
        super.onDraw(canvas);canvas.save();canvas.scale(density,density);canvas.drawColor(colors.canvas);hits.clear();
        if(books.isEmpty()){float y=Math.max(0,height()*.28f-24);if(height()>=170){round(canvas,new RectF(width()/2-32,y,width()/2+32,y+64),20,colors.panel);icon(canvas,bookIcon,width()/2-14,y+18,28);centered(canvas,emptyTitle,y+98,width()-40,20,colors.ink);centered(canvas,emptyNote,y+126,width()-40,13,colors.muted);}else{centered(canvas,emptyTitle,32,width()-40,18,colors.ink);centered(canvas,emptyNote,57,width()-40,12,colors.muted);}canvas.restore();return;}
        flat(canvas);canvas.restore();
    }
    private void flat(Canvas c){
        float w=width(),cw=ShelfGeometry.cellWidth(w);int first=ShelfGeometry.first(w,books.size(),mode,offset),end=ShelfGeometry.end(w,height(),books.size(),mode,offset);
        for(int i=first;i<end;i++){
            ShelfStore.Book b=books.get(i);ShelfGeometry.Tile t=new ShelfGeometry.Tile(w,mode,i,offset);float x=t.left,y=t.top;RectF rect=new RectF(x,y,t.right,t.bottom),menu=new RectF(t.menuLeft,t.menuTop,t.menuRight,t.menuBottom);
            if(i==pressed)round(c,rect,12,colors.selected);
            if(mode==ShelfGeometry.LIST){
                cover(c,b,new RectF(x,y+8,x+56,y+88));float tx=x+72;label(c,b.label(),tx,y+28,t.right-tx-48,16,colors.ink,true);
                label(c,b.author.isEmpty()?b.name:b.author,tx,y+51,t.right-tx-8,12,colors.muted,false);
                label(c,(b.pinned?"置顶 · ":"")+b.format,tx,y+75,(t.right-tx)*.55f,11,colors.muted,false);right(c,b.progress(),t.right-8,y+75,(t.right-tx)*.45f,11,colors.accent);
                bar(c,tx,y+89,t.right-tx-8,b.percent);paint.setColor(colors.border);c.drawRect(tx,y+103,t.right,y+103.5f,paint);
            }else{
                float base=y+cw*ShelfGeometry.COVER_RATIO;cover(c,b,new RectF(x,y,x+cw,base));
                titleLines(c,b.label(),x,base+23,cw,15);label(c,b.format,x,base+64,cw*.45f,11,colors.muted,false);right(c,b.progress(),t.right,base+64,cw*.55f,11,colors.accent);bar(c,x,base+73,cw,b.percent);
                round(c,new RectF(menu.centerX()-15,menu.centerY()-15,menu.centerX()+15,menu.centerY()+15),15,colors.panel);
                if(b.pinned){round(c,new RectF(x+8,y+9,x+32,y+33),8,colors.panel);icon(c,pinIcon,x+13,y+14,14);}
            }
            icon(c,moreIcon,menu.centerX()-10,menu.centerY()-10,20);hits.add(new Hit(i,rect,menu));
        }
        float maximum=max();if(maximum>0){float sh=Math.min(height(),Math.max(26,height()*height()/(maximum+height()))),sy=(height()-sh)*offset/maximum;round(c,new RectF(w-4,sy,w-2,sy+sh),1,colors.border);}
    }
    private void cover(Canvas c,ShelfStore.Book book,RectF r){
        Bitmap image=covers.get(book.id);c.save();coverClip.rewind();coverClip.addRoundRect(r,8,8,Path.Direction.CW);c.clipPath(coverClip);round(c,r,8,colors.panel);
        if(image!=null){paint.setColor(Color.WHITE);c.drawRect(r,paint);float scale=Math.min(r.width()/image.getWidth(),r.height()/image.getHeight());float ww=image.getWidth()*scale,hh=image.getHeight()*scale;RectF fit=new RectF(r.centerX()-ww/2,r.centerY()-hh/2,r.centerX()+ww/2,r.centerY()+hh/2);paint.setColor(Color.WHITE);c.drawBitmap(image,null,fit,paint);}
        else {
            paint.setColor(colors.accent);c.drawRect(r.left,r.top,r.left+Math.max(3,r.width()*.045f),r.bottom,paint);
            float pad=r.width()*.12f;float size=Math.min(23,r.width()*.13f);String text=book.label();
            label(c,book.format,r.left+pad,r.top+r.height()*.17f,r.width()-pad*2,Math.max(6,size*.55f),colors.muted);
            for(int line=0;line<3&&!text.isEmpty();line++){
                paint.setTypeface(ShelfStyle.MEDIUM);paint.setTextSize(size);int n=paint.breakText(text,true,r.width()-pad*2,null);if(n>0&&n<text.length()&&Character.isHighSurrogate(text.charAt(n-1)))n--;if(n<=0)break;
                label(c,text.substring(0,n),r.left+pad,r.top+r.height()*.41f+line*size*1.4f,r.width()-pad*2,size,colors.ink);text=text.substring(n);
            }
            label(c,"READALL",r.left+pad,r.bottom-r.height()*.10f,r.width()-pad*2,Math.max(6,size*.48f),colors.muted);
        }
        paint.setColor((colors.border&0x00ffffff)|0x85000000);paint.setStyle(Paint.Style.STROKE);paint.setStrokeWidth(.7f);c.drawRoundRect(r,8,8,paint);paint.setStyle(Paint.Style.FILL);c.restore();
    }
    private void round(Canvas c,RectF r,float radius,int color){paint.setColor(color);paint.setStyle(Paint.Style.FILL);c.drawRoundRect(r,radius,radius,paint);}
    private void bar(Canvas c,float x,float y,float w,double progress){round(c,new RectF(x,y,x+w,y+2),1,colors.border);if(progress>=0)round(c,new RectF(x,y,x+w*(float)(progress/100),y+2),1,colors.accent);}
    private String fit(String text,float width){if(paint.measureText(text)<=width)return text;int n=paint.breakText(text,true,Math.max(0,width-paint.measureText("…")),null);if(n>0&&n<text.length()&&Character.isHighSurrogate(text.charAt(n-1)))n--;return text.substring(0,n)+"…";}
    private void label(Canvas c,String text,float x,float y,float width,float size,int color){label(c,text,x,y,width,size,color,false);}
    private void label(Canvas c,String text,float x,float y,float width,float size,int color,boolean medium){paint.setTypeface(medium?ShelfStyle.MEDIUM:ShelfStyle.REGULAR);paint.setTextSize(size);paint.setColor(color);paint.setStyle(Paint.Style.FILL);c.drawText(fit(text.replace('\n',' '),Math.max(0,width)),x,y,paint);}
    private void right(Canvas c,String text,float right,float y,float width,float size,int color){paint.setTypeface(ShelfStyle.REGULAR);paint.setTextSize(size);String value=fit(text,Math.max(0,width));label(c,value,right-paint.measureText(value),y,width,size,color);}
    private void centered(Canvas c,String text,float y,float width,float size,int color){paint.setTypeface(ShelfStyle.REGULAR);paint.setTextSize(size);String value=fit(text,Math.max(0,width));label(c,value,(this.width()-paint.measureText(value))/2,y,width,size,color);}
    private void titleLines(Canvas c,String text,float x,float y,float width,float size){text=text.replace('\n',' ');paint.setTypeface(ShelfStyle.MEDIUM);paint.setTextSize(size);int n=paint.breakText(text,true,Math.max(0,width),null);if(n>0&&n<text.length()&&Character.isHighSurrogate(text.charAt(n-1)))n--;if(n==0)return;label(c,text.substring(0,n),x,y,width,size,colors.ink,true);if(n<text.length())label(c,text.substring(n).trim(),x,y+21,width,size,colors.ink,true);}
    private void icon(Canvas c,ShelfIcon icon,float x,float y,int size){int save=c.save();c.translate(x,y);icon.setBounds(0,0,size,size);icon.draw(c);c.restoreToCount(save);}
}
