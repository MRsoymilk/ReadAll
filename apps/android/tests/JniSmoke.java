package xin.soymilk.readall;

import java.io.File;
import java.nio.ByteBuffer;
import java.nio.file.Files;
import java.security.MessageDigest;
import java.util.Arrays;
import java.util.function.Predicate;

/** Runs the actual native library through a JVM with -Xcheck:jni. No Android stubs. */
public final class JniSmoke {
    static void require(boolean value,String message) { if(!value)throw new AssertionError(message); }
    static NativeReader.State waitFor(NativeReader reader,Predicate<NativeReader.State> done) throws Exception {
        long deadline=System.nanoTime()+30000000000L;
        while(true) {
            NativeReader.State state=reader.state();
            if(done.test(state))return state;
            require(!state.closed(),"native reader stopped: "+state.notice);
            require(System.nanoTime()<deadline,"native reader timeout: "+state.phase+" "+state.notice);
            Thread.sleep(10);
        }
    }
    public static void main(String[] args) throws Exception {
        File root=new File(args[0]).getCanonicalFile(),book=new File(root,"book.epub"),font=new File(root,"font.ttf"),state=new File(root,"state");
        byte[] before=MessageDigest.getInstance("SHA-256").digest(Files.readAllBytes(book.toPath()));
        String anchor;
        try(NativeReader reader=new NativeReader(book.getPath(),font.getPath(),state.getPath(),400,520,16,24)) {
            NativeReader.State first=waitFor(reader,s->s.serial>0&&!s.busy());
            ByteBuffer pixels=ByteBuffer.allocateDirect(first.byteLength());
            require(reader.copyPixels(first,pixels),"initial frame copy failed");
            int colors=0;for(int i=0;i<pixels.capacity();i+=4)if((pixels.get(i)&255)==10&&(pixels.get(i+1)&255)==90&&(pixels.get(i+2)&255)==180&&(pixels.get(i+3)&255)==255)colors++;
            require(colors>0,"real image pixels did not cross JNI in RGBA order");
            require("expanded".equals(first.uiMode),"shared toolbar not enabled");
            reader.command(NativeReader.TOUCH,200,344);reader.command(NativeReader.TOUCH+1,200,344);waitFor(reader,s->"collapsed".equals(s.uiMode));
            reader.command(NativeReader.TOUCH,200,493);reader.command(NativeReader.TOUCH+1,200,493);waitFor(reader,s->"expanded".equals(s.uiMode));
            boolean rejected=false;try{reader.copyPixels(first,ByteBuffer.allocate(4));}catch(IllegalArgumentException expected){rejected=true;}require(rejected,"heap/small buffer accepted");
            rejected=false;try{reader.command(999);}catch(IllegalStateException expected){rejected=true;}require(rejected,"unknown native opcode accepted");
            reader.command(NativeReader.NEXT);NativeReader.State second=waitFor(reader,s->!s.locator.equals(first.locator)&&!s.busy());require(!second.locator.equals(first.locator),"next did not navigate");
            pixels.put(0,(byte)77);require(!reader.copyPixels(first,pixels),"stale frame must not be copied");require(pixels.get(0)==77,"stale frame changed destination");
            reader.command(NativeReader.CONTENTS);
            waitFor(reader,s->!s.busy()&&reader.contents().length==2);
            NativeReader.Item[] contents=reader.contents();require(contents[1].spine==1,"wrong chapter mapping");
            reader.command(NativeReader.JUMP,contents[1].spine,contents[1].offset);
            NativeReader.State last=waitFor(reader,s->s.position.contains("第 2/2 章")&&!s.busy());anchor=last.locator;
            reader.command(NativeReader.RESIZE,500,600);NativeReader.State resized=waitFor(reader,s->s.width==500&&!s.busy());require(anchor.equals(resized.locator),"resize lost the anchor");
            reader.command(NativeReader.THEME);waitFor(reader,s->s.serial>resized.serial&&!s.busy());
            reader.command(NativeReader.BOOKMARK);waitFor(reader,s->s.notice.contains("书签已保存"));
            reader.command(NativeReader.SETTINGS);waitFor(reader,s->"settings".equals(s.uiMode));
            for(int i=0;i<4;i++)reader.command(NativeReader.NEXT);reader.command(NativeReader.LARGER);waitFor(reader,s->"book".equals(s.pageMode));
            reader.command(NativeReader.LARGER);waitFor(reader,s->"scroll".equals(s.pageMode));reader.command(NativeReader.LARGER);waitFor(reader,s->"slide".equals(s.pageMode));
            reader.command(NativeReader.BACK);waitFor(reader,s->"expanded".equals(s.uiMode));
            reader.command(NativeReader.FIND);waitFor(reader,s->s.editing);reader.input("search","中文😀");waitFor(reader,s->s.input.equals("中文😀"));
            reader.command(NativeReader.PASTE);String[] effects=new String[0];long deadline=System.nanoTime()+5000000000L;
            while(effects.length==0&&System.nanoTime()<deadline){effects=reader.effects();Thread.sleep(5);}
            require(effects.length==2&&effects[0].equals("paste"),"clipboard request not routed to Android");require(reader.effects().length==0,"host request replayed");
            reader.hostReply(2," World");waitFor(reader,s->s.input.equals("中文😀 World"));reader.command(NativeReader.BACK);waitFor(reader,s->!s.editing);
            reader.command(NativeReader.PAUSE,1,0);reader.command(NativeReader.PAUSE,0,0);
        }
        Thread.sleep(150);
        try(NativeReader resumed=new NativeReader(book.getPath(),font.getPath(),state.getPath(),500,600,16,24)) {
            NativeReader.State ready=waitFor(resumed,s->s.serial>0&&!s.busy());require(anchor.equals(ready.locator),"saved progress did not restore");
            resumed.close();resumed.close();boolean closed=false;try{resumed.state();}catch(IllegalStateException expected){closed=true;}require(closed,"closed handle remained accessible");
        }
        require(Arrays.equals(before,MessageDigest.getInstance("SHA-256").digest(Files.readAllBytes(book.toPath()))),"input book changed");
        System.out.println("PASS real JVM/JNI: publication load, RGBA pixels, bounded buffer checks, errors, navigation, shared toolbar/contents/settings, all page modes, UTF-8 input, host effects, resize, theme, bookmark, restart and idempotent close; source unchanged");
    }
}
