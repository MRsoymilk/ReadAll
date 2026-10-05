package xin.soymilk.readall;

import android.graphics.Canvas;
import android.graphics.ColorFilter;
import android.graphics.Paint;
import android.graphics.Path;
import android.graphics.PixelFormat;
import android.graphics.drawable.Drawable;

/** Small original line icons. Vector paths are built once, never downloaded or font glyphs. */
final class ShelfIcon extends Drawable {
    static final int ADD=0,SEARCH=1,CLOSE=2,LIST=3,GRID=4,SORT=5,SUN=6,MOON=7,BOOK=8,ARROW=9,PIN=10,EDIT=11,REFRESH=12,TRASH=13,MORE=14;
    private final Path path=new Path();
    private final Paint paint=new Paint(Paint.ANTI_ALIAS_FLAG);
    private final int size,color;
    private int alpha=255;
    ShelfIcon(int kind,int color,int size){
        this.size=size;this.color=color;paint.setStyle(Paint.Style.STROKE);paint.setStrokeWidth(1.7f);paint.setStrokeCap(Paint.Cap.ROUND);paint.setStrokeJoin(Paint.Join.ROUND);
        switch(kind){
            case ADD:line(12,5,12,19);line(5,12,19,12);break;
            case SEARCH:path.addCircle(10.5f,10.5f,6.5f,Path.Direction.CW);line(15.5f,15.5f,21,21);break;
            case CLOSE:line(6,6,18,18);line(18,6,6,18);break;
            case LIST:for(int y=6;y<=18;y+=6){line(4,y,5,y);line(9,y,20,y);}break;
            case GRID:for(int x=4;x<=14;x+=10)for(int y=4;y<=14;y+=10)path.addRoundRect(x,y,x+6,y+6,1,1,Path.Direction.CW);break;
            case SORT:line(5,6,19,6);line(5,12,15,12);line(5,18,11,18);break;
            case SUN:path.addCircle(12,12,4,Path.Direction.CW);for(int n=0;n<8;n++){double a=n*Math.PI/4;line(12+7*(float)Math.cos(a),12+7*(float)Math.sin(a),12+9*(float)Math.cos(a),12+9*(float)Math.sin(a));}break;
            case MOON:path.moveTo(19.8f,15);path.cubicTo(15,17,7,10,10,3.5f);path.cubicTo(-1,6,3,24,15,20);path.quadTo(18,19,19.8f,15);break;
            case BOOK:path.moveTo(12,6);path.cubicTo(9,3,5,3,3,4);path.lineTo(3,19);path.cubicTo(6,18,9,18,12,21);path.cubicTo(15,18,18,18,21,19);path.lineTo(21,4);path.cubicTo(18,3,15,3,12,6);line(12,6,12,21);break;
            case ARROW:line(5,12,19,12);line(14,7,19,12);line(19,12,14,17);break;
            case PIN:path.moveTo(8,3);path.lineTo(16,3);path.lineTo(15,10);path.lineTo(19,15);path.lineTo(5,15);path.lineTo(9,10);path.close();line(12,15,12,22);break;
            case EDIT:path.moveTo(4,16);path.lineTo(16,4);path.lineTo(20,8);path.lineTo(8,20);path.lineTo(3,21);path.close();line(13,7,17,11);break;
            case REFRESH:path.addArc(4,4,20,20,40,290);line(20,4,20,10);line(14,10,20,10);break;
            case TRASH:line(4,6,20,6);path.moveTo(8,6);path.lineTo(8,3);path.lineTo(16,3);path.lineTo(16,6);path.moveTo(6,6);path.lineTo(7,21);path.lineTo(17,21);path.lineTo(18,6);line(10,10,10,17);line(14,10,14,17);break;
            case MORE:for(int x=5;x<=19;x+=7)path.addCircle(x,12,.75f,Path.Direction.CW);break;
            default:throw new IllegalArgumentException("Unknown shelf icon");
        }
        setBounds(0,0,size,size);
    }
    private void line(float x,float y,float endX,float endY){path.moveTo(x,y);path.lineTo(endX,endY);}
    @Override public void draw(Canvas canvas){int save=canvas.save();canvas.translate(getBounds().left,getBounds().top);canvas.scale(getBounds().width()/24f,getBounds().height()/24f);paint.setColor(color);paint.setAlpha(alpha);canvas.drawPath(path,paint);canvas.restoreToCount(save);}
    @Override public void setAlpha(int value){alpha=Math.max(0,Math.min(255,value));invalidateSelf();}
    @Override public void setColorFilter(ColorFilter filter){paint.setColorFilter(filter);invalidateSelf();}
    @Override public int getOpacity(){return PixelFormat.TRANSLUCENT;}
    @Override public int getIntrinsicWidth(){return size;}
    @Override public int getIntrinsicHeight(){return size;}
}
