package xin.soymilk.readall;

/** Logical-unit list/grid layout and bounded visible ranges; independent of Android. */
final class ShelfGeometry {
    static final int LIST=0,COVERS=1;
    // Keep existing IDs stable; the removed mode (2) and unknown values become covers.
    static int mode(int value){return value==LIST?LIST:COVERS;}
    static int columns(float width){return Math.max(1,Math.min(6,(int)((Math.max(1,width)-18)/150)));}
    static float cellWidth(float width){int cols=columns(width);return Math.max(32,(width-32-(cols-1)*14)/cols);}
    static float pitch(float width,int mode){return mode==LIST?108:cellWidth(width)*1.40f+80;}
    static float maxOffset(float width,float height,int count,int mode){int rows=mode==LIST?count:(count+columns(width)-1)/columns(width);return Math.max(0,rows*pitch(width,mode)+20-height);}
    static float clamp(float value,float max){return Float.isFinite(value)?Math.max(0,Math.min(max,value)):0;}
    private static double rowPosition(float offset,float pitch){double units=(double)offset/pitch,nearest=Math.rint(units);return Math.abs(units-nearest)<0.0001?nearest:units;}
    static int first(float width,int count,int mode,float offset){if(count==0)return 0;return Math.min(count,(int)rowPosition(offset,pitch(width,mode))*(mode==LIST?1:columns(width)));}
    static int end(float width,float height,int count,int mode,float offset){int rows=(int)Math.ceil(height/pitch(width,mode))+2;return Math.min(count,first(width,count,mode,offset)+rows*(mode==LIST?1:columns(width)));}
    /** Keep the same book visible across modes and width/column changes. */
    static float restoreOffset(float width,int mode,int index,float previousWidth,int previousMode,float previousOffset){
        if(index<0)return 0;
        double units=rowPosition(previousOffset,pitch(previousWidth,previousMode));
        float fraction=mode==previousMode?(float)(units-Math.floor(units)):0;
        int row=mode==LIST?index:index/columns(width);return (row+fraction)*pitch(width,mode);
    }
    private ShelfGeometry(){}
}
