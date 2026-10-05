package xin.soymilk.readall;

import java.io.File;
import java.nio.ByteBuffer;
import java.lang.reflect.Modifier;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.TimeUnit;

/** Real JNI pixels and concurrent input; no mocked Android drawing or device FPS claim. */
public final class SmoothScrollSmoke {
    static ByteBuffer pixels(NativeReader r) throws Exception {
        for(int i=0;i<100;i++) { NativeReader.State s=r.state();ByteBuffer b=ByteBuffer.allocateDirect(s.byteLength());if(s.serial>0&&!s.busy()&&r.copyPixels(s,b))return b;Thread.sleep(10); }
        throw new AssertionError("cannot capture stable frame");
    }
    public static void main(String[] args) throws Exception {
        JniSmoke.require(!Modifier.isSynchronized(NativeReader.class.getMethod("copyPixels",NativeReader.State.class,ByteBuffer.class).getModifiers()),"pixel copy must not lock UI command/state methods");
        File root=new File(args[0]);
        try(NativeReader r=new NativeReader(new File(root,"toc.epub").getPath(),new File(root,"font.ttf").getPath(),new File(root,"smooth-state").getPath(),400,640,16,24,1080,1728)) {
            NativeReader.State first=JniSmoke.waitFor(r,s->s.serial>0&&!s.busy());
            r.command(NativeReader.CONTENTS);JniSmoke.waitFor(r,s->"toc".equals(s.uiMode)&&!s.busy());
            r.command(NativeReader.PAUSE,1,0);Thread.sleep(40);
            NativeReader.State still=r.state();ByteBuffer before=pixels(r);
            TouchRouter touch=new TouchRouter(8,(kind,x,y)->r.command(NativeReader.TOUCH+kind,x,y));
            touch.down(200,260);touch.move(200,250);touch.move(200,248);touch.up(200,248,0);
            JniSmoke.waitFor(r,s->s.serial>still.serial&&!s.busy());Thread.sleep(40);
            ByteBuffer after=pixels(r);
            int changes=0;for(int i=0;i<after.capacity();i++)if(after.get(i)!=before.get(i))changes++;
            JniSmoke.require(changes>100,"12-unit drag did not move any rows (old row snapping)");
            JniSmoke.require(r.state().locator.equals(first.locator),"TOC drag changed reading anchor");
            NativeReader.State copyState=r.state();ByteBuffer buffer=ByteBuffer.allocateDirect(copyState.byteLength());
            CompletableFuture<Void> copying=CompletableFuture.runAsync(()->{for(int i=0;i<20;i++)r.copyPixels(copyState,buffer);});
            for(int i=0;i<20;i++){r.state();r.command(NativeReader.SAVE);}
            copying.get(15,TimeUnit.SECONDS);
            r.command(NativeReader.PAUSE,0,0);
            touch.down(200,260);touch.move(200,248);touch.up(200,248,150);
            JniSmoke.waitFor(r,s->s.animating&&"toc".equals(s.uiMode));
            touch.down(200,248);
            JniSmoke.waitFor(r,s->!s.animating&&"toc".equals(s.uiMode)&&!s.busy());
            touch.cancel();
            JniSmoke.require(r.state().locator.equals(first.locator),"TOC inertia moved the book");
        }
        System.out.println("PASS real JNI: sub-row pixels, TOC inertia/stop, high-density shared frames and concurrent copy/input");
    }
}
