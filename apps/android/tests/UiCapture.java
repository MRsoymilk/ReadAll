package xin.soymilk.readall;
import java.io.File;
import java.nio.ByteBuffer;
import java.awt.image.BufferedImage;
import javax.imageio.ImageIO;

/** Inspect shared reader pixels with a supplied local font; never opens a user's book. */
public final class UiCapture {
    private static void save(NativeReader reader,File path) throws Exception {
        for(int attempt=0;attempt<100;attempt++){
            NativeReader.State s=reader.state();
            if(s.serial>0&&!s.busy()){
                ByteBuffer b=ByteBuffer.allocateDirect(s.byteLength());
                if(reader.copyPixels(s,b)){
                    BufferedImage image=new BufferedImage(s.width,s.height,BufferedImage.TYPE_INT_ARGB);
                    for(int y=0;y<s.height;y++)for(int x=0;x<s.width;x++){int r=b.get()&255,g=b.get()&255,blue=b.get()&255,a=b.get()&255;image.setRGB(x,y,(a<<24)|(r<<16)|(g<<8)|blue);}
                    ImageIO.write(image,"png",path);System.out.println(path+" "+s.uiMode+" "+s.pageMode);return;
                }
            }
            Thread.sleep(20);
        }
        throw new AssertionError("capture timed out");
    }
    public static void main(String[] args) throws Exception {
        File root=new File(args[0]),font=new File(args[1]),out=new File(root,"screens");out.mkdirs();
        int width=args.length>2?Integer.parseInt(args[2]):400,height=args.length>3?Integer.parseInt(args[3]):800;
        try(NativeReader reader=new NativeReader(new File(root,"book.epub").getPath(),font.getPath(),new File(root,"capture-state-"+width+"-"+height).getPath(),width,height,20,20)){
            JniSmoke.waitFor(reader,s->s.serial>0&&!s.busy());long revision=reader.state().revision;reader.command(NativeReader.PAUSE,1,0);JniSmoke.waitFor(reader,s->s.revision>revision&&!s.busy());
            save(reader,new File(out,width+"-expanded.png"));
            reader.command(NativeReader.TOUCH+1,width/2,height-176);JniSmoke.waitFor(reader,s->s.uiMode.equals("collapsed"));save(reader,new File(out,width+"-collapsed.png"));
            reader.command(NativeReader.CONTENTS);JniSmoke.waitFor(reader,s->s.uiMode.equals("toc"));save(reader,new File(out,width+"-toc.png"));
            reader.command(NativeReader.SETTINGS);JniSmoke.waitFor(reader,s->s.uiMode.equals("settings"));save(reader,new File(out,width+"-settings.png"));
        }
    }
}
