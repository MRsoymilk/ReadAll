package xin.soymilk.readall;

import java.io.File;
import java.nio.ByteBuffer;

/** Real JNI menu frames/actions at two densities, without Android framework mocks. */
public final class ReaderMenuSmoke {
    static NativeReader.State send(NativeReader r,int code,int x,int y)throws Exception {long serial=r.state().serial;r.command(code,x,y);return JniSmoke.waitFor(r,s->s.serial>serial&&!s.busy());}
    public static void main(String[] args)throws Exception {
        File root=new File(args[0]);
        for(boolean dense:new boolean[]{false,true})try(NativeReader r=new NativeReader(new File(root,"book.epub").getPath(),new File(root,"font.ttf").getPath(),new File(root,"menu-state-"+dense).getPath(),400,640,20,16,dense?1080:400,dense?1728:640)){
            NativeReader.State initial=JniSmoke.waitFor(r,s->s.serial>0&&!s.busy());String anchor=initial.locator;send(r,NativeReader.PAUSE,1,0);send(r,NativeReader.THEME,0,0);send(r,NativeReader.THEME,0,0);
            NativeReader.State settings=send(r,NativeReader.SETTINGS,0,0);JniSmoke.require(settings.uiMode.equals("settings")&&!settings.notice.contains("↑"),"mobile settings still show desktop hints");
            ByteBuffer image=ByteBuffer.allocateDirect(settings.byteLength());JniSmoke.require(r.copyPixels(settings,image),"menu pixels unavailable");
            int top=Math.round(40f*settings.height/640f);JniSmoke.require(ThemeSmoke.color(image,settings.width/2,top,settings.width)==settings.appearance.panel,"settings lost its themed panel");
            JniSmoke.require(ThemeSmoke.color(image,Math.round(12f*settings.width/400f),top,settings.width)!=settings.appearance.panel,"settings corner was not rounded");
            send(r,NativeReader.TOUCH+1,45,186); // Select the size label only.
            String config=new String(java.nio.file.Files.readAllBytes(new File(root,"menu-state-"+dense+"/library-v1/settings.conf").toPath()),java.nio.charset.StandardCharsets.UTF_8);
            // The two explicit theme changes above persist defaults without changing size.
            JniSmoke.require(config.contains("size=20\n"),"label decremented font size");
            send(r,NativeReader.TOUCH+1,348,186);send(r,NativeReader.TOUCH+1,300,186);
            JniSmoke.require(r.state().locator.equals(anchor),"font roundtrip lost reading position");
            send(r,NativeReader.TOUCH+1,366,58);JniSmoke.require(r.state().uiMode.equals("expanded"),"close target changed");
            NativeReader.State find=send(r,NativeReader.FIND,0,0);JniSmoke.require(!find.notice.contains("Enter"),"search has desktop-only hint");
            long serial=find.serial;r.input("search","AAAA");JniSmoke.waitFor(r,s->s.serial>serial&&!s.busy());NativeReader.State result=send(r,NativeReader.TOUCH+1,344,98);JniSmoke.require(result.notice.contains("个结果"),"search submit target failed");
            send(r,NativeReader.BACK,0,0);NativeReader.State note=send(r,NativeReader.NOTE,0,0);JniSmoke.require(!note.notice.contains("Ctrl"),"note has desktop-only hint");send(r,NativeReader.DISMISS,0,0);
            send(r,NativeReader.CONTENTS,0,0);JniSmoke.require(r.state().uiMode.equals("toc"),"TOC unavailable");
        }
        System.out.println("PASS real JNI modern menus: rounded themed frames, high density, explicit setting controls, search/note/TOC and unchanged reading anchors");
    }
}
