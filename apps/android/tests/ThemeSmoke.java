package xin.soymilk.readall;
import java.io.File;
import java.nio.ByteBuffer;
import java.nio.file.Files;

/** Actual JNI palette, persistent home preference and shared reader theme roundtrip. */
public final class ThemeSmoke {
    static void check(boolean ok,String why){if(!ok)throw new AssertionError(why);}
    static int color(ByteBuffer b,int x,int y,int width){int i=(y*width+x)*4;return ((b.get(i+3)&255)<<24)|((b.get(i)&255)<<16)|((b.get(i+1)&255)<<8)|(b.get(i+2)&255);}
    public static void main(String[] args) throws Exception {
        File root=new File(args[0]),state=new File(root,"theme-smoke-state");
        NativeReader.Appearance light=NativeReader.appearance("light"),dark=NativeReader.appearance("dark");
        check(!light.dark()&&dark.dark()&&light.panel!=dark.panel,"light/dark palette missing");
        check("light".equals(NativeReader.appearance("paper").name),"legacy paper not accepted");
        check("light".equals(NativeReader.appearance("sepia").name),"legacy sepia not accepted");
        boolean rejected=false;try{NativeReader.appearance("invalid");}catch(IllegalStateException e){rejected=true;}check(rejected,"unknown theme accepted");
        NativeReader.saveTheme(state.getPath(),"dark");
        check(NativeReader.loadAppearance(state.getPath()).dark(),"home theme was not saved");
        File settings=new File(state,"library-v1/settings.conf");String before=new String(Files.readAllBytes(settings.toPath()),java.nio.charset.StandardCharsets.UTF_8);
        check(before.contains("size=20\n")&&before.contains("margin=16\n"),"home theme changed new-phone defaults");
        String anchor;
        try(NativeReader r=new NativeReader(new File(root,"book.epub").getPath(),new File(root,"font.ttf").getPath(),state.getPath(),400,640,20,16,1080,1728)){
            NativeReader.State s=JniSmoke.waitFor(r,v->v.serial>0&&!v.busy());anchor=s.locator;
            check(s.appearance.dark(),"reader ignored home theme");
            for(String name:new String[]{"light","dark"}){
                r.command(NativeReader.THEME);s=JniSmoke.waitFor(r,v->v.appearance.name.equals(name)&&!v.busy());
                check(s.locator.equals(anchor),"theme moved reading position");
                long revision=s.revision;r.command(NativeReader.PAUSE,1,0);s=JniSmoke.waitFor(r,v->v.revision>revision&&!v.busy());
                ByteBuffer bytes=ByteBuffer.allocateDirect(s.byteLength());check(r.copyPixels(s,bytes),"theme frame copy failed");
                check(color(bytes,0,0,s.width)==s.appearance.page,"header did not use shared page color");
                check(color(bytes,s.width/2,1220,s.width)==s.appearance.panel,"toolbar retained old fixed color");
                check(color(bytes,38,1220,s.width)!=s.appearance.panel,"rounded toolbar corner should reveal the page");
                r.command(NativeReader.SETTINGS);JniSmoke.waitFor(r,v->v.uiMode.equals("settings")&&!v.busy());r.command(NativeReader.BACK);JniSmoke.waitFor(r,v->v.uiMode.equals("expanded")&&!v.busy());
            }
        }
        check(NativeReader.loadAppearance(state.getPath()).dark(),"reader theme did not reach home storage");
        NativeReader.saveTheme(state.getPath(),"light");
        String after=new String(Files.readAllBytes(settings.toPath()),java.nio.charset.StandardCharsets.UTF_8);
        check(before.replace("theme=dark","theme=light").equals(after),"theme changed unrelated preferences");
        try(NativeReader r=new NativeReader(new File(root,"book.epub").getPath(),new File(root,"font.ttf").getPath(),state.getPath(),400,640,20,16)){
            NativeReader.State s=JniSmoke.waitFor(r,v->v.serial>0&&!v.busy());check(!s.appearance.dark()&&s.locator.equals(anchor),"theme/progress did not restore");
        }
        System.out.println("PASS real JNI: shared light/dark palette, home/reader persistence, high-density chrome, legacy names and unchanged progress/settings");
    }
}
