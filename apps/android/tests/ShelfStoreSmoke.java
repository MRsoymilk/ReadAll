package xin.soymilk.readall;

import java.io.*;
import java.nio.file.*;
import java.util.*;
import java.util.concurrent.*;

/** Real disk/concurrent mutation tests; never uses the user's library or Android stubs. */
public final class ShelfStoreSmoke {
    static void check(boolean b,String message){if(!b)throw new AssertionError(message);}
    static ShelfStore.Book book(int n,String title,long time)throws Exception {return new ShelfStore.Book(String.format(Locale.ROOT,"%064x.epub",n),title+".epub",title,"作者","EPUB","content://fixture/"+n,"",time,0,-1,false);}
    public static void main(String[] args)throws Exception {
        Path tmp=Files.createTempDirectory("readall-shelf-test-");
        try{
            ShelfStore a=new ShelfStore(tmp.toFile()),b=new ShelfStore(tmp.toFile());ShelfStore.Book one=book(1,"中文书籍😀",1),two=book(2,"Second book",2);
            a.add(one);a.add(two);check(a.load().size()==2,"add missing");a.opened(one.id,10);ShelfStore.Book migrated=b.load().get(0);check(migrated.opened==10&&migrated.percent==-1&&migrated.progress().equals("待恢复进度"),"migration guessed progress or lost continue entry");a.rename(one.id,"自定义标题");a.pin(one.id,true);a.progress(one.id,42.5,20);a.add(book(1,"原书更新",4));
            ShelfStore.Book restored=b.load().get(0);check(restored.label().equals("自定义标题")&&restored.percent==42.5&&restored.pinned&&restored.added==1,"duplicate import lost reader state or alias");
            check(ShelfStore.select(b.load(),"自定义","title").size()==1,"search missed alias");check(ShelfStore.select(b.load(),"作者","recent").size()==2,"search missed author");check(ShelfStore.select(b.load(),"","recent").get(0).id.equals(one.id),"pin ordering lost");
            Files.write(a.bookFile(one).toPath(),new byte[]{1,2,3});Files.write(tmp.resolve("notes.keep"),new byte[]{7});a.remove(Collections.singleton(one.id));b.progress(one.id,90,30);check(a.load().size()==1,"late progress resurrected removed book");check(Files.exists(a.bookFile(one).toPath())&&Files.exists(tmp.resolve("notes.keep")),"index removal deleted book/annotations");
            ExecutorService workers=Executors.newFixedThreadPool(2);try{Future<?> x=workers.submit(()->{try{for(int i=10;i<20;i++)a.add(book(i,"A "+i,i));}catch(Exception e){throw new RuntimeException(e);}});Future<?> y=workers.submit(()->{try{for(int i=20;i<30;i++)b.add(book(i,"B "+i,i));}catch(Exception e){throw new RuntimeException(e);}});x.get(10,TimeUnit.SECONDS);y.get(10,TimeUnit.SECONDS);}finally{workers.shutdownNow();}
            check(a.load().size()==21,"concurrent store instances lost writes");
            Path index=tmp.resolve("shelf-v1.bin");byte[] valid=Files.readAllBytes(index),bad="foreign index".getBytes(java.nio.charset.StandardCharsets.UTF_8);Files.write(index,bad);boolean rejected=false;try{a.add(one);}catch(IOException e){rejected=true;}check(rejected&&Arrays.equals(Files.readAllBytes(index),bad),"corrupt index silently overwritten");Files.write(index,valid);check(new ShelfStore(tmp.toFile()).load().size()==21,"restart lost entries");
            rejected=false;try{new ShelfStore.Book("../outside.epub","x","x","x","EPUB","","",0,0,-1,false);}catch(IOException e){rejected=true;}check(rejected,"path traversal accepted");
            rejected=false;try{a.progress(two.id,Double.NaN,2);}catch(IOException e){rejected=true;}check(rejected,"invalid progress accepted");
            for(float width:new float[]{256,320,393,848,1200})for(float height:new float[]{128,256,848})for(int count:new int[]{0,1,5,1000})for(int mode=ShelfGeometry.LIST;mode<=ShelfGeometry.COVERS;mode++){
                float max=ShelfGeometry.maxOffset(width,height,count,mode);check(max>=0&&Float.isFinite(max),"bad extent");for(float offset:new float[]{0,max/2,max}){int first=ShelfGeometry.first(width,count,mode,offset),end=ShelfGeometry.end(width,height,count,mode,offset);check(first>=0&&end>=first&&end<=count,"visible range invalid");int columns=mode==ShelfGeometry.LIST?1:ShelfGeometry.columns(width);check(end-first<=((int)Math.ceil(height/ShelfGeometry.pitch(width,mode))+2)*columns,"visible list/grid range unbounded");}
            }
            for(float oldWidth:new float[]{1,320,393,848})for(float width:new float[]{320,393,848})for(int from=ShelfGeometry.LIST;from<=ShelfGeometry.COVERS;from++)for(int to=ShelfGeometry.LIST;to<=ShelfGeometry.COVERS;to++){
                int anchorIndex=23;float old=ShelfGeometry.restoreOffset(oldWidth,from,anchorIndex,oldWidth,from,0),restoredOffset=ShelfGeometry.restoreOffset(width,to,anchorIndex,oldWidth,from,old);
                int first=ShelfGeometry.first(width,1000,to,restoredOffset);check(Float.isFinite(restoredOffset)&&restoredOffset>=0,"resize offset invalid");
                check(first<=anchorIndex&&anchorIndex<first+(to==ShelfGeometry.LIST?1:ShelfGeometry.columns(width)),"focused book lost when columns change");
            }
            check(ShelfGeometry.restoreOffset(393,ShelfGeometry.COVERS,-1,1,ShelfGeometry.COVERS,999)==0,"removed focus should recover to start");
            check(ShelfGeometry.LIST==0&&ShelfGeometry.COVERS==1,"existing view IDs must remain stable");
            check(ShelfGeometry.mode(0)==ShelfGeometry.LIST&&ShelfGeometry.mode(1)==ShelfGeometry.COVERS,"valid views changed");
            for(int savedMode:new int[]{2,-1,99,Integer.MIN_VALUE,Integer.MAX_VALUE}){
                int next=ShelfGeometry.mode(savedMode);check(next==ShelfGeometry.COVERS,"removed/unknown view must fall back to covers");
                for(float width:new float[]{256,320,393,848})for(int anchorIndex:new int[]{0,1,23,999}){
                    float offset=ShelfGeometry.restoreOffset(width,next,anchorIndex,width,next,0);int first=ShelfGeometry.first(width,1000,next,offset);
                    check(first<=anchorIndex&&anchorIndex<first+ShelfGeometry.columns(width),"legacy focus lost after view fallback");
                }
            }
            check(Arrays.equals(Files.readAllBytes(index),valid),"view changes must not rewrite books or reading state");
            System.out.println("PASS bookshelf: atomic persistence, deduplication, alias/pin/search/sort, progress, safe removal, corruption refusal, concurrent writers, bounded list/grid geometry and removed-view fallback");
        }finally{try(java.util.stream.Stream<Path> paths=Files.walk(tmp)){paths.sorted(Comparator.reverseOrder()).forEach(p->{try{Files.deleteIfExists(p);}catch(IOException e){throw new UncheckedIOException(e);}});}}
    }
}
