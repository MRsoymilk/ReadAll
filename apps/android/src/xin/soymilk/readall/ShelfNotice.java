package xin.soymilk.readall;

/** UI-thread transient notice policy, with an injectable monotonic clock for regression tests. */
final class ShelfNotice {
    static final int TIMEOUT_MS=6000;
    private String text="";
    private boolean working;
    private long deadline=-1;
    void update(String value,boolean busy,long now,int timeout){
        text=value==null?"":value;working=busy;
        deadline=working||text.isEmpty()?-1:now+Math.max(TIMEOUT_MS,timeout);
    }
    String text(){return text;}
    boolean visible(){return working||!text.isEmpty();}
    boolean working(){return working;}
    long remaining(long now){return deadline<0?-1:Math.max(0,deadline-now);}
    void expire(long now){if(!working&&deadline>=0&&now>=deadline)clear();}
    // Dismissal/backgrounding must not hide active work or lose its Cancel action.
    void dismiss(){if(!working)clear();}
    private void clear(){text="";deadline=-1;}
}
