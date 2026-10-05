package xin.soymilk.readall;
import java.io.File;
import java.nio.ByteBuffer;
/** Uses the real native library at independent layout/output sizes. */
public final class DensitySmoke {
    public static void main(String[] args) throws Exception {
        File root=new File(args[0]);String book=new File(root,"book.epub").getPath(),font=new File(root,"font.ttf").getPath();
        try(NativeReader low=new NativeReader(book,font,new File(root,"low-density-state").getPath(),400,520,16,16);
            NativeReader high=new NativeReader(book,font,new File(root,"high-density-state").getPath(),400,520,16,16,1080,1404)){
            NativeReader.State a=JniSmoke.waitFor(low,s->s.serial>0&&!s.busy());
            NativeReader.State b=JniSmoke.waitFor(high,s->s.serial>0&&!s.busy());
            JniSmoke.require(a.locator.equals(b.locator)&&a.position.equals(b.position),"density altered logical pagination");
            JniSmoke.require(b.width==1080&&b.height==1404&&b.logicalWidth==400&&b.logicalHeight==520,"logical and pixel geometry confused");
            ByteBuffer pixels=ByteBuffer.allocateDirect(b.byteLength());JniSmoke.require(high.copyPixels(b,pixels),"native-density transfer failed");
            JniSmoke.require(pixels.capacity()==1080*1404*4,"wrong buffer stride");
            high.command(NativeReader.TOUCH,200,344);high.command(NativeReader.TOUCH+1,200,344);JniSmoke.waitFor(high,s->"collapsed".equals(s.uiMode));
            high.viewport(400,520,800,1040);NativeReader.State resized=JniSmoke.waitFor(high,s->s.width==800&&!s.busy());
            JniSmoke.require(resized.locator.equals(b.locator)&&resized.logicalWidth==400,"density resize changed the anchor");
            System.out.println("PASS real JNI high-density pixels, same logical pagination, touch targets and density-only resize");
        }
    }
}
