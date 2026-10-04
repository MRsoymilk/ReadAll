package xin.soymilk.readall;

import java.nio.ByteBuffer;

/** Narrow JNI contract. One native actor per open book; no Android types required. */
public final class NativeReader implements AutoCloseable {
    static { System.loadLibrary("readall_android"); }
    public static final int NEXT=1, PREVIOUS=2, FIRST=3, LAST=4, LARGER=5, SMALLER=6, CONTENTS=7, JUMP=8, THEME=9, SAVE=10, BOOKMARK=11, RESIZE=12;
    private long handle;
    public NativeReader(String book, String font, String state, int width, int height, int size, int margin) {
        handle=nativeOpen(book,font,state,width,height,size,margin);
        if(handle<=0) throw new IllegalStateException("ReadAll native reader creation failed");
    }
    private void requireOpen() { if(handle==0) throw new IllegalStateException("ReadAll reader closed"); }
    public synchronized State state() { requireOpen();return new State(nativeState(handle)); }
    public synchronized void command(int code) { command(code,0,0); }
    public synchronized void command(int code,int a,int b) { requireOpen();nativeCommand(handle,code,a,b); }
    /** Caller owns target exclusively until this method returns. Position is ignored. */
    public synchronized boolean copyPixels(State state, ByteBuffer target) {
        requireOpen();
        if(!target.isDirect() || target.isReadOnly() || target.capacity()<state.byteLength()) throw new IllegalArgumentException("writable direct frame buffer required");
        return nativeCopyPixels(handle,state.serial,target);
    }
    public synchronized Item[] contents() {
        requireOpen();String[] fields=nativeContents(handle);
        if(fields==null || fields.length%4!=0) throw new IllegalStateException("invalid native contents response");
        Item[] items=new Item[fields.length/4];
        for(int i=0;i<items.length;i++) items[i]=new Item(fields[i*4],Integer.parseInt(fields[i*4+1]),Integer.parseInt(fields[i*4+2]),Integer.parseInt(fields[i*4+3]));
        return items;
    }
    @Override public synchronized void close() { if(handle!=0) { long value=handle;handle=0;nativeClose(value); } }
    public static final class Item {
        public final String title;public final int depth,spine,offset;
        Item(String title,int depth,int spine,int offset) { this.title=title;this.depth=depth;this.spine=spine;this.offset=offset; }
    }
    public static final class State {
        public final String status,phase,title,position,percent,locator,notice;
        public final long done,total,serial,revision;
        public final int width,height;
        State(String[] f) {
            if(f==null || f.length!=14 || !"1".equals(f[0])) throw new IllegalStateException("unsupported ReadAll native protocol");
            status=f[1];phase=f[2];done=Long.parseLong(f[3]);total=Long.parseLong(f[4]);serial=Long.parseLong(f[5]);width=Integer.parseInt(f[6]);height=Integer.parseInt(f[7]);title=f[8];position=f[9];percent=f[10];locator=f[11];notice=f[12];revision=Long.parseLong(f[13]);
            if(width<0 || height<0 || (long)width*height>4194304L) throw new IllegalStateException("invalid native frame geometry");
        }
        public boolean busy() { return "loading".equals(status); }
        public boolean closed() { return "closed".equals(status); }
        public int byteLength() { return Math.toIntExact((long)width*height*4); }
    }
    private static native long nativeOpen(String book,String font,String state,int width,int height,int size,int margin);
    private static native String[] nativeState(long handle);
    private static native String[] nativeContents(long handle);
    private static native void nativeCommand(long handle,int code,int a,int b);
    private static native boolean nativeCopyPixels(long handle,long serial,ByteBuffer buffer);
    private static native void nativeClose(long handle);
}
