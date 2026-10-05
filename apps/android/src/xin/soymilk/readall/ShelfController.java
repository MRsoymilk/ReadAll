package xin.soymilk.readall;

import android.app.Activity;
import android.app.AlertDialog;
import android.content.SharedPreferences;
import android.graphics.Bitmap;
import android.net.Uri;
import android.os.Handler;
import android.os.Looper;
import android.view.ContextThemeWrapper;
import android.widget.CheckBox;
import android.widget.EditText;
import java.io.*;
import java.nio.ByteBuffer;
import java.nio.file.Files;
import java.nio.file.StandardCopyOption;
import java.security.MessageDigest;
import java.util.*;
import java.util.concurrent.ExecutorService;

/** One IO owner for shelf mutations; the UI holds immutable book records only. */
final class ShelfController implements ShelfHome.Callbacks {
    interface Host {void pickBooks();void toggleTheme();void openBook(File file,String name,String uri,String id);}
    private final Activity activity;
    private final ExecutorService io;
    private final Handler main=new Handler(Looper.getMainLooper());
    private final Host host;
    private final SharedPreferences prefs;
    private final File directory;
    private final ShelfCoverCache cache;
    final ShelfHome view;
    private ShelfStore store;
    private ByteBuffer preview;
    private List<ShelfStore.Book> books=Collections.emptyList();
    private NativeReader.Appearance appearance;
    private volatile int generation;
    private volatile boolean closed;
    private boolean busy;
    private int mode;
    private String sort;
    ShelfController(Activity activity,ExecutorService io,NativeReader.Appearance appearance,Host host){
        this.activity=activity;this.io=io;this.appearance=appearance;this.host=host;directory=new File(activity.getFilesDir(),"bookshelf-v1");prefs=activity.getSharedPreferences("bookshelf",Activity.MODE_PRIVATE);
        mode=ShelfGeometry.mode(prefs.getInt("mode",ShelfGeometry.COVERS));sort=prefs.getString("sort","recent");
        cache=new ShelfCoverCache(new File(directory,"covers"),this::invalidate);
        view=new ShelfHome(activity,cache,appearance,mode,sort,this);
    }
    private void invalidate(){if(!closed)view.canvas.invalidate();}
    void load(){io.execute(()->{try{if(store==null)store=new ShelfStore(directory);try{migrate();}catch(Exception migration){error(migration);}List<ShelfStore.Book> data=store.load();main.post(()->{if(!closed){books=data;view.books(data,prefs.getString("focus",""));}});}catch(Exception e){error(e);}});}
    private void reload(){try{List<ShelfStore.Book> data=store.load();main.post(()->{if(!closed){books=data;view.books(data,null);}});}catch(Exception e){error(e);}}
    private void error(Throwable e){String message=e.getMessage()==null?e.getClass().getSimpleName():e.getMessage();main.post(()->{if(!closed)view.message("书库："+message);});}
    private void migrate()throws Exception {
        if(prefs.getBoolean("migrated-last-v1",false))return;
        SharedPreferences legacy=activity.getSharedPreferences("library",Activity.MODE_PRIVATE);String path=legacy.getString("book","");
        if(!path.isEmpty()){
            File old=new File(path).getCanonicalFile(),legacyRoot=new File(activity.getCacheDir(),"books").getCanonicalFile();
            if(old.isFile()&&old.getParentFile().equals(legacyRoot)&&old.getName().matches("[0-9a-f]{64}\\.(epub|mobi)")&&old.length()<=BookFiles.MAX_BOOK){
                File target=new File(store.books,old.getName());BookFiles.checkRoom(store.books,target,old.length());
                if(!target.exists()){
                    File temp=File.createTempFile("migration-",".tmp",store.books);
                    try{Files.copy(old.toPath(),temp.toPath(),StandardCopyOption.REPLACE_EXISTING);if(!sha256(temp).equals(old.getName().substring(0,64)))throw new IOException("旧缓存标识不匹配；原文件保留");Files.move(temp.toPath(),target.toPath(),StandardCopyOption.ATOMIC_MOVE);}finally{Files.deleteIfExists(temp.toPath());}
                }
                List<ShelfStore.Book> rows=store.load();boolean present=false;for(ShelfStore.Book b:rows)if(b.file.equals(target.getName()))present=true;
                if(!present){ShelfStore.Book migrated=accept(target,legacy.getString("name","图书"),legacy.getString("uri",""));store.opened(migrated.id,Math.max(1,old.lastModified()));}
            }
        }
        prefs.edit().putBoolean("migrated-last-v1",true).apply();
    }
    private static String sha256(File file)throws Exception {MessageDigest d=MessageDigest.getInstance("SHA-256");try(InputStream in=new FileInputStream(file)){byte[] b=new byte[65536];int n;while((n=in.read(b))!=-1)d.update(b,0,n);}StringBuilder hex=new StringBuilder(64);for(byte b:d.digest())hex.append(String.format(Locale.ROOT,"%02x",b&255));return hex.toString();}
    File booksDirectory(){return new File(directory,"books");}
    /** Called on the existing IO executor by import/retry paths. Never invokes reader layout. */
    ShelfStore.Book accept(File file,String name,String uri)throws Exception {return accept(file,name,uri,()->false);}
    private ShelfStore.Book accept(File file,String name,String uri,BookFiles.Cancelled cancelled)throws Exception {
        if(cancelled.get())throw new IOException("导入已取消");
        if(store==null)store=new ShelfStore(directory);
        if(!file.getCanonicalFile().getParentFile().equals(store.books.getCanonicalFile()))throw new IOException("只能登记应用管理的图书副本");
        if(preview==null)preview=ByteBuffer.allocateDirect(NativeReader.PREVIEW_BYTES);
        NativeReader.Preview p=NativeReader.preview(file.getAbsolutePath(),preview);
        if(cancelled.get())throw new IOException("导入已取消");
        ShelfStore.Book book=new ShelfStore.Book(file.getName(),safe(name,512),safe(p.title,512),safe(p.author,512),p.format,safe(uri,8192),"",System.currentTimeMillis(),0,-1,false);
        try{if(p.width>0){
            Bitmap image=Bitmap.createBitmap(p.width,p.height,Bitmap.Config.ARGB_8888);File temporary=File.createTempFile("cover-",".tmp",store.covers);
            try{preview.position(0);image.copyPixelsFromBuffer(preview);try(FileOutputStream out=new FileOutputStream(temporary)){if(!image.compress(Bitmap.CompressFormat.PNG,100,out))throw new IOException("无法写入封面");out.getFD().sync();}Files.move(temporary.toPath(),store.coverFile(book).toPath(),StandardCopyOption.ATOMIC_MOVE,StandardCopyOption.REPLACE_EXISTING);}finally{image.recycle();Files.deleteIfExists(temporary.toPath());}
        }}catch(IOException|OutOfMemoryError thumbnail){error(new IOException("封面缓存不可用，将使用文字封面"));}
        if(cancelled.get())throw new IOException("导入已取消");
        store.add(book);main.post(()->{if(!closed)cache.refresh(book.id);});return book;
    }
    static String safe(String s,int n){if(s==null)return "";s=s.replace('\0',' ');if(s.length()>n){if(Character.isHighSurrogate(s.charAt(n-1)))n--;s=s.substring(0,n);}return s;}
    void importBooks(List<Uri> uris){
        if(busy||uris.isEmpty())return;busy=true;int token=++generation;view.busy(true,"准备导入 "+uris.size()+" 本图书…");
        io.execute(()->{
            int succeeded=0,failed=0;String lastError="";
            try{
                if(store==null)store=new ShelfStore(directory);
                for(int i=0;i<uris.size();i++){
                    if(closed||token!=generation)break;
                    final int number=i+1;final long[] last={0};Uri uri=uris.get(i);
                    try{
                        BookFiles.Imported result=BookFiles.read(activity.getContentResolver(),uri,store.books,null,(done,total)->{long now=System.nanoTime();if(now-last[0]>80000000L){last[0]=now;main.post(()->{if(!closed&&token==generation)view.message("导入 "+number+" / "+uris.size()+" · "+(done/1024/1024)+" MiB");});}},()->closed||token!=generation||Thread.currentThread().isInterrupted());
                        if(closed||token!=generation)break;
                        main.post(()->{if(!closed&&token==generation)view.message("读取书名与封面 · "+number+" / "+uris.size());});
                        accept(result.file,result.name,uri.toString(),()->closed||token!=generation||Thread.currentThread().isInterrupted());succeeded++;reload();
                    }catch(Exception|LinkageError|OutOfMemoryError e){if(token!=generation)break;failed++;lastError=safe(e.getMessage(),160);}
                }
                int done=succeeded,errors=failed;String why=lastError;
                main.post(()->{if(!closed&&token==generation){busy=false;view.busy(false,"已加入 "+done+" 本"+(errors>0?"，失败 "+errors+" 本："+why:" · 重复图书自动合并"));}});
            }catch(Exception e){main.post(()->{if(!closed&&token==generation){busy=false;view.busy(false,"导入未完成，已有图书保留");}});error(e);}
        });
    }
    boolean busy(){return busy;}
    @Override public void cancelImport(){if(!busy)return;++generation;busy=false;view.busy(false,"导入已取消，已完成的图书保留");}
    @Override public void add(){if(!busy)host.pickBooks();}
    @Override public void theme(){if(!busy)host.toggleTheme();}
    @Override public void mode(int value){mode=ShelfGeometry.mode(value);prefs.edit().putInt("mode",mode).apply();}
    @Override public void sort(String value){sort=value;prefs.edit().putString("sort",sort).apply();}
    @Override public void focused(String id){prefs.edit().putString("focus",id).apply();}
    @Override public void open(ShelfStore.Book book){
        if(busy){view.message("正在导入；可先取消导入再阅读");return;}view.clearSearch();view.canvas.stop();
        io.execute(()->{try{File file=store.bookFile(book);if(!file.isFile())throw new IOException("副本已丢失，请重新导入此书；进度和标注保留");main.post(()->{if(!closed&&!busy)host.openBook(file,book.label(),book.uri,book.id);});}catch(Exception e){error(e);}});
    }
    boolean openLast(){ShelfStore.Book last=null;for(ShelfStore.Book b:books)if(last==null||b.opened>last.opened)last=b;if(last==null)return false;open(last);return true;}
    private ContextThemeWrapper dialogContext(){return new ContextThemeWrapper(activity,appearance.dark()?android.R.style.Theme_Material_Dialog_Alert:android.R.style.Theme_Material_Light_Dialog_Alert);}
    @Override public void menu(ShelfStore.Book book){
        if(busy)return;view.canvas.stop();String[] actions={"打开阅读",book.pinned?"取消置顶":"置顶图书","修改显示书名","重新读取封面","移出书库"};
        new AlertDialog.Builder(dialogContext()).setTitle(book.label()).setItems(actions,(dialog,which)->{
            if(which==0){open(book);return;}if(which==1){change(()->store.pin(book.id,!book.pinned));return;}
            if(which==2){EditText input=new EditText(dialogContext());input.setSingleLine();input.setText(book.label());input.setFilters(new android.text.InputFilter[]{new android.text.InputFilter.LengthFilter(512)});new AlertDialog.Builder(dialogContext()).setTitle("修改显示书名").setMessage("只修改书架显示，不修改原书。留空恢复书籍标题。").setView(input).setNegativeButton("取消",null).setPositiveButton("保存",(d,w)->change(()->store.rename(book.id,input.getText().toString()))).show();return;}
            if(which==3){change(()->accept(store.bookFile(book),book.name,book.uri));return;}
            CheckBox clear=new CheckBox(dialogContext());clear.setText("同时清理应用内副本和封面");clear.setPadding(48,12,24,12);
            new AlertDialog.Builder(dialogContext()).setTitle("移出书库？").setMessage(book.label()+"\n\n不会删除系统中的原书，也不会清除阅读进度、书签、高亮或笔记。").setView(clear).setNegativeButton("取消",null).setPositiveButton("移除",(d,w)->{final boolean purge=clear.isChecked();change(()->{store.remove(Collections.singleton(book.id));SharedPreferences legacy=activity.getSharedPreferences("library",Activity.MODE_PRIVATE);if(new File(legacy.getString("book","")).getName().equals(book.file))legacy.edit().remove("book").remove("uri").remove("name").apply();if(purge){try{Files.deleteIfExists(store.bookFile(book).toPath());Files.deleteIfExists(store.coverFile(book).toPath());}catch(IOException cleanup){error(new IOException("记录已移除，但副本清理失败"));}}main.post(()->{if(!closed)cache.refresh(book.id);});});}).show();
        }).show();
    }
    interface Operation {void run()throws Exception;}
    private void change(Operation op){io.execute(()->{try{op.run();reload();main.post(()->{if(!closed)view.message("书库已更新");});}catch(Exception e){error(e);}});}
    void progress(String id,String percent){if(id==null||id.isEmpty())return;double value;try{value=Double.parseDouble(percent);}catch(RuntimeException ignored){return;}if(!Double.isFinite(value))return;final double p=value;io.execute(()->{try{if(store==null)store=new ShelfStore(directory);store.progress(id,p,System.currentTimeMillis());}catch(Exception e){error(e);}});}
    void theme(NativeReader.Appearance value){appearance=value;view.theme(value);}
    void resume(){load();}
    void pause(){view.canvas.stop();}
    void close(){closed=true;++generation;cache.close();view.canvas.stop();}
}
