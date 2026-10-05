package xin.soymilk.readall;

import java.io.File;
import java.nio.file.Files;
import java.security.MessageDigest;
import java.util.Arrays;

/** Actual TouchRouter -> JNI -> shared TOC viewport -> tapped chapter regression. */
public final class TocDragSmoke {
    public static void main(String[] args) throws Exception {
        File root=new File(args[0]),book=new File(root,"toc.epub"),font=new File(root,"font.ttf");
        byte[] before=MessageDigest.getInstance("SHA-256").digest(Files.readAllBytes(book.toPath()));
        int cases=0;
        for(boolean dense:new boolean[]{false,true})for(int scenario=0;scenario<4;scenario++){
            File state=new File(root,"toc-drag-state-"+dense+"-"+scenario);
            int expected=scenario==2?1:scenario==3?22:2;
            try(NativeReader r=new NativeReader(book.getPath(),font.getPath(),state.getPath(),400,640,16,24,dense?1080:400,dense?1728:640)){
                NativeReader.State first=JniSmoke.waitFor(r,s->s.serial>0&&!s.busy());
                r.command(NativeReader.CONTENTS);JniSmoke.waitFor(r,s->"toc".equals(s.uiMode)&&!s.busy());
                NativeReader.Item[] entries=r.contents();JniSmoke.require(entries.length==30,"long TOC fixture missing");
                TouchRouter touch=new TouchRouter(8,(kind,x,y)->r.command(NativeReader.TOUCH+kind,x,y));
                touch.down(200,260);
                if(scenario==0){touch.move(200,184);touch.up(200,184,600);}
                else if(scenario==1){touch.move(200,146);touch.move(200,184);touch.up(200,184,600);}
                else if(scenario==2){touch.move(200,460);touch.move(200,422);touch.up(200,422,-600);}
                else {touch.move(200,-9740);touch.move(200,-9740);touch.move(200,-9702);touch.up(200,-9702,600);}
                // At 400x640 the shared TOC starts at y=88 with 7 visible 38-unit rows.
                // The queued tap activates the new first visible entry, never keyboard focus.
                touch.down(200,148);touch.up(200,148,0);
                NativeReader.State jumped=JniSmoke.waitFor(r,s->!s.locator.equals(first.locator)&&!s.busy());
                JniSmoke.require(jumped.position.contains("第 "+(entries[expected].spine+1)+"/30 章"),"wrong chapter after drag: "+scenario+" dense="+dense+" "+jumped.position);
                JniSmoke.require("expanded".equals(jumped.uiMode),"tap did not close the TOC");
                cases++;
            }
        }
        JniSmoke.require(Arrays.equals(before,MessageDigest.getInstance("SHA-256").digest(Files.readAllBytes(book.toPath()))),"fixture book changed");
        System.out.println("PASS "+cases+" real TouchRouter/JNI TOC cases: up/down content direction, edge reversal, coalesced movement, high density and tapped chapter");
    }
}
