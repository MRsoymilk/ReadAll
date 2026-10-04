package xin.soymilk.readall;

/** Platform-free gesture state machine. All coordinates are shared-renderer pixels. */
final class TouchRouter {
    interface Sink { void send(int kind,int x,int y); }
    private final Sink sink;
    private final float slop;
    private int mode; // 0 idle, 1 undecided, 2 page/list drag, 3 long-press selection.
    private int startX,startY,lastX,lastY;
    TouchRouter(float slop,Sink sink) { this.slop=slop;this.sink=sink; }
    boolean active() { return mode!=0; }
    boolean selecting() { return mode==3; }
    void down(int x,int y) {
        if(mode!=0)cancel();
        mode=1;startX=lastX=x;startY=lastY=y;sink.send(0,x,y);
    }
    void move(int x,int y) {
        if(mode==0)return;
        lastX=x;lastY=y;
        if(mode==1 && (Math.abs((long)x-startX)>=slop || Math.abs((long)y-startY)>=slop)) {
            mode=2;sink.send(2,startX,startY);
        }
        if(mode==2)sink.send(3,x,y);
        else if(mode==3)sink.send(6,x,y);
    }
    void longPress() { if(mode==1) { mode=3;sink.send(5,startX,startY); } }
    void up(int x,int y,int flingY) {
        if(mode==0)return;
        // Classify final displacement even when Android coalesces/misses a MOVE.
        if(mode==1)move(x,y);
        if(mode==1)sink.send(1,x,y);
        else if(mode==2) { sink.send(3,x,y);sink.send(4,x,y);if(flingY!=0)sink.send(9,0,flingY); }
        else if(mode==3) { sink.send(6,x,y);sink.send(7,x,y); }
        mode=0;
    }
    void cancel() { if(mode!=0)sink.send(8,lastX,lastY);mode=0; }
}
