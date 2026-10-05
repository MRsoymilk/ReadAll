package xin.soymilk.readall;

import java.nio.ByteBuffer;

/** Narrow JNI contract. One native actor per open book; no Android types required. */
public final class NativeReader implements AutoCloseable {
    static { System.loadLibrary("readall_android"); }
    public static final int NEXT=1, PREVIOUS=2, FIRST=3, LAST=4, LARGER=5, SMALLER=6, CONTENTS=7, JUMP=8, THEME=9, SAVE=10, BOOKMARK=11, RESIZE=12;
    public static final int BACK=13, PAUSE=14, FIND=20, ANNOTATIONS=21, SETTINGS=22, SELECT=23, COPY=24, PASTE=25, NOTE=26, HIGHLIGHT=27, DELETE=28, ACTIVATE=29, DISMISS=30, BACKSPACE=31, TOUCH=40;
    private long handle;
    public NativeReader(String book, String font, String state, int width, int height, int size, int margin) {
        this(book,font,state,width,height,size,margin,width,height);
    }
    public NativeReader(String book, String font, String state, int width, int height, int size, int margin, int pixelWidth, int pixelHeight) {
        handle=nativeOpen(book,font,state,width,height,size,margin,pixelWidth,pixelHeight);
        if(handle<=0) throw new IllegalStateException("ReadAll native reader creation failed");
    }
    private void requireOpen() { if(handle==0) throw new IllegalStateException("ReadAll reader closed"); }
    public synchronized State state() { requireOpen();return new State(nativeState(handle)); }
    public synchronized void command(int code) { command(code,0,0); }
    public synchronized void command(int code,int a,int b) { requireOpen();nativeCommand(handle,code,a,b); }
    private synchronized long checkedHandle() { requireOpen();return handle; }
    /** Caller owns target exclusively until return. Native captures an immutable frame
     * before copying; do not hold this monitor over megabytes of pixels and block UI input. */
    public boolean copyPixels(State state, ByteBuffer target) {
        long owner=checkedHandle();
        if(!target.isDirect() || target.isReadOnly() || target.capacity()<state.byteLength()) throw new IllegalArgumentException("writable direct frame buffer required");
        return nativeCopyPixels(owner,state.serial,target);
    }
    public synchronized Item[] contents() {
        requireOpen();String[] fields=nativeContents(handle);
        if(fields==null || fields.length%4!=0) throw new IllegalStateException("invalid native contents response");
        Item[] items=new Item[fields.length/4];
        for(int i=0;i<items.length;i++) items[i]=new Item(fields[i*4],Integer.parseInt(fields[i*4+1]),Integer.parseInt(fields[i*4+2]),Integer.parseInt(fields[i*4+3]));
        return items;
    }
    public synchronized void viewport(int width,int height,int pixelWidth,int pixelHeight) { requireOpen();nativeViewport(handle,width,height,pixelWidth,pixelHeight); }
    public synchronized void input(String mode,String text) { requireOpen();nativeInput(handle,mode,text); }
    public synchronized void hostReply(int kind,String text) { requireOpen();nativeHostReply(handle,kind,text); }
    public synchronized String[] effects() { requireOpen();String[] f=nativeEffects(handle);if(f==null||f.length%2!=0)throw new IllegalStateException("invalid host effects");return f; }
    @Override public synchronized void close() { if(handle!=0) { long value=handle;handle=0;nativeClose(value); } }
    public static final class Item {
        public final String title;public final int depth,spine,offset;
        Item(String title,int depth,int spine,int offset) { this.title=title;this.depth=depth;this.spine=spine;this.offset=offset; }
    }
    /** Colors are supplied by the same Rust palette used to draw Linux/Android reader UI. */
    public static final class Appearance {
        public final String name;
        public final int canvas,page,panel,ink,muted,border,accent,onAccent,button,selected,hover;
        Appearance(String[] f,int offset) {
            if(f==null||f.length!=offset+12||!("light".equals(f[offset])||"dark".equals(f[offset])))throw new IllegalStateException("invalid theme protocol");
            name=f[offset];int[] c=new int[11];
            for(int i=0;i<c.length;i++){long v=Long.parseLong(f[offset+1+i]);if(v<0xff000000L||v>0xffffffffL)throw new IllegalStateException("invalid theme color");c[i]=(int)v;}
            canvas=c[0];page=c[1];panel=c[2];ink=c[3];muted=c[4];border=c[5];accent=c[6];onAccent=c[7];button=c[8];selected=c[9];hover=c[10];
        }
        public boolean dark(){return "dark".equals(name);}
    }
    /** Pure lookup; does not read or write settings. */
    public static Appearance appearance(String name){return new Appearance(nativeAppearance("",name),0);}
    /** Call disk-backed appearance operations only on the Android IO executor. */
    public static Appearance loadAppearance(String state){return new Appearance(nativeAppearance(state,""),0);}
    public static Appearance saveTheme(String state,String name){return new Appearance(nativeAppearance(state,name),0);}
    public static final class State {
        public final String status,phase,title,position,percent,locator,notice,uiMode,pageMode,input;
        public final boolean animating,editing;
        public final Appearance appearance;
        public final long done,total,serial,revision;
        public final int width,height,logicalWidth,logicalHeight;
        State(String[] f) {
            if(f==null || f.length!=33 || !"4".equals(f[0])) throw new IllegalStateException("unsupported ReadAll native protocol");
            status=f[1];phase=f[2];done=Long.parseLong(f[3]);total=Long.parseLong(f[4]);serial=Long.parseLong(f[5]);width=Integer.parseInt(f[6]);height=Integer.parseInt(f[7]);title=f[8];position=f[9];percent=f[10];locator=f[11];notice=f[12];revision=Long.parseLong(f[13]);
            uiMode=f[14];pageMode=f[15];animating="1".equals(f[16]);editing="1".equals(f[17]);input=f[18];
            logicalWidth=Integer.parseInt(f[19]);logicalHeight=Integer.parseInt(f[20]);appearance=new Appearance(f,21);
            if(width<0 || height<0 || logicalWidth<0 || logicalHeight<0 || (long)width*height>4194304L) throw new IllegalStateException("invalid native frame geometry");
        }
        public boolean busy() { return "loading".equals(status); }
        public boolean closed() { return "closed".equals(status); }
        public int byteLength() { return Math.toIntExact((long)width*height*4); }
    }
    private static native long nativeOpen(String book,String font,String state,int width,int height,int size,int margin,int pixelWidth,int pixelHeight);
    private static native void nativeViewport(long handle,int width,int height,int pixelWidth,int pixelHeight);
    private static native String[] nativeState(long handle);
    private static native String[] nativeContents(long handle);
    private static native void nativeCommand(long handle,int code,int a,int b);
    private static native boolean nativeCopyPixels(long handle,long serial,ByteBuffer buffer);
    private static native void nativeClose(long handle);
    private static native void nativeInput(long handle,String mode,String text);
    private static native void nativeHostReply(long handle,int kind,String text);
    private static native String[] nativeEffects(long handle);
    private static native String[] nativeAppearance(String state,String requested);
}
