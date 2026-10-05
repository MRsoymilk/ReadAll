package xin.soymilk.readall;
public final class ViewportSmoke {
    static void check(boolean condition,String why){if(!condition)throw new AssertionError(why);}
    public static void main(String[] args){
        int[] v=ReaderViewport.sizes(1080,2400,2.75f);
        check(v[0]==393 && v[1]==873 && v[2]==1080 && v[3]==2400,"phone pixels reduced to layout units");
        int[] p=ReaderViewport.point(540,1200,1080,2400,1080,2400,v[0],v[1]);
        check(Math.abs(p[0]-v[0]/2f)<=0.5f&&Math.abs(p[1]-v[1]/2f)<=0.5f,"native-pixel hit transform");
        for(int[] screen:new int[][]{{1080,2160},{2400,1080},{7680,4320},{320,600}}){
            v=ReaderViewport.sizes(screen[0],screen[1],2.75f);check((long)v[2]*v[3]<=ReaderViewport.MAX_PIXELS,"unbounded frame allocation");
            p=ReaderViewport.point(screen[0]/2f,screen[1]/2f,screen[0],screen[1],v[2],v[3],v[0],v[1]);
            check(Math.abs(p[0]-v[0]/2f)<=0.51f && Math.abs(p[1]-v[1]/2f)<=0.51f,"resized/letterboxed input mapping");
        }
        boolean rejected=false;try{ReaderViewport.sizes(0,200,1);}catch(IllegalArgumentException e){rejected=true;}check(rejected,"invalid zero viewport");
        System.out.println("PASS native pixel dimensions, bounded fallback, logical touch coordinates and orientation mapping");
    }
}
