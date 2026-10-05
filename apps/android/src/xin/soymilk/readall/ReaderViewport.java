package xin.soymilk.readall;

/** Layout units never double as bitmap pixels. The common reader keeps its Linux
 * geometry; normal phones receive one raster pixel per available screen pixel. */
final class ReaderViewport {
    static final long MAX_PIXELS=4L*1024*1024;
    static int[] sizes(int width,int height,float density) {
        if(width<=0||height<=0||width>16384||height>16384||!Float.isFinite(density)||density<=0)throw new IllegalArgumentException("Invalid viewport");
        int lw=Math.max(320,Math.min(1024,Math.round(width/Math.max(1,density))));
        int lh=Math.max(256,Math.min(2048,Math.round(height*(float)lw/width)));
        double scale=Math.min(1.0,Math.sqrt(MAX_PIXELS/((double)width*height)));
        int pw=Math.max(1,(int)Math.floor(width*scale)),ph=Math.max(1,(int)Math.floor(height*scale));
        return new int[]{lw,lh,pw,ph};
    }
    static int[] point(float x,float y,int viewWidth,int viewHeight,int pixelWidth,int pixelHeight,int logicalWidth,int logicalHeight) {
        float scale=Math.min((float)viewWidth/pixelWidth,(float)viewHeight/pixelHeight);
        float left=(viewWidth-pixelWidth*scale)/2,top=(viewHeight-pixelHeight*scale)/2;
        return new int[]{Math.round((x-left)/scale*logicalWidth/pixelWidth),Math.round((y-top)/scale*logicalHeight/pixelHeight)};
    }
}
