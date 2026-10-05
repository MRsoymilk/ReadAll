package xin.soymilk.readall;

import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.os.Handler;
import android.os.Looper;
import android.util.LruCache;
import java.io.File;
import java.util.HashSet;
import java.util.Set;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

/** Small immutable thumbnails only; decoder IO never executes inside onDraw. */
final class ShelfCoverCache {
    private final File directory;
    private final Handler main=new Handler(Looper.getMainLooper());
    private final ExecutorService worker=Executors.newSingleThreadExecutor();
    private final Set<String> pending=new HashSet<>(),missing=new HashSet<>();
    private final java.util.Map<String,Integer> revisions=new java.util.HashMap<>();
    private final LruCache<String,Bitmap> cache=new LruCache<String,Bitmap>(20*1024*1024){@Override protected int sizeOf(String key,Bitmap image){return image.getAllocationByteCount();}};
    private final Runnable changed;
    private boolean closed;
    ShelfCoverCache(File directory,Runnable changed){this.directory=directory;this.changed=changed;}
    Bitmap get(String id){
        Bitmap image=cache.get(id);if(image!=null||closed||missing.contains(id)||pending.contains(id)||pending.size()>=12)return image;
        if(!id.matches("[0-9a-f]{64}"))return null;
        final int revision=revisions.getOrDefault(id,0);pending.add(id);worker.execute(()->{
            Bitmap result=null;
            try{
                File file=new File(directory,id+".png");
                if(file.isFile()&&file.length()<=2*1024*1024){
                    BitmapFactory.Options bounds=new BitmapFactory.Options();bounds.inJustDecodeBounds=true;BitmapFactory.decodeFile(file.getPath(),bounds);
                    if(bounds.outWidth>0&&bounds.outHeight>0&&bounds.outWidth<=384&&bounds.outHeight<=512){
                        BitmapFactory.Options options=new BitmapFactory.Options();options.inPreferredConfig=Bitmap.Config.ARGB_8888;result=BitmapFactory.decodeFile(file.getPath(),options);
                        if(result!=null)result.setDensity(Bitmap.DENSITY_NONE);
                    }
                }
            }catch(RuntimeException|OutOfMemoryError ignored){}
            Bitmap ready=result;main.post(()->{pending.remove(id);if(closed){if(ready!=null)ready.recycle();return;}if(revision!=revisions.getOrDefault(id,0)){if(ready!=null)ready.recycle();changed.run();return;}if(ready==null)missing.add(id);else cache.put(id,ready);changed.run();});
        });return null;
    }
    void refresh(String id){revisions.put(id,revisions.getOrDefault(id,0)+1);cache.remove(id);missing.remove(id);changed.run();}
    void close(){closed=true;worker.shutdownNow();cache.evictAll();missing.clear();revisions.clear();}
}
