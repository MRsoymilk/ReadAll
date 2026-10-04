package xin.soymilk.readall;

import android.content.ContentResolver;
import android.database.Cursor;
import android.net.Uri;
import android.provider.OpenableColumns;
import android.content.res.AssetManager;
import java.io.*;
import java.security.MessageDigest;
import java.util.Arrays;
import java.util.Comparator;

/** Only caller-granted content URIs; never attempts to derive a filesystem path. */
final class BookFiles {
    static final long MAX_BOOK=128L*1024*1024;
    interface Progress { void update(long done,long total); }
    interface Cancelled { boolean get(); }
    static final class Imported {
        final File file;final String name;
        Imported(File file,String name) { this.file=file;this.name=name; }
    }
    static Imported read(ContentResolver resolver,Uri uri,File cache,File preserve,Progress progress,Cancelled cancelled) throws Exception {
        if(!"content".equals(uri.getScheme())) throw new IOException("请选择系统文件选择器中的图书");
        String name="图书";long total=0;
        try(Cursor cursor=resolver.query(uri,new String[]{OpenableColumns.DISPLAY_NAME,OpenableColumns.SIZE},null,null,null)) {
            if(cursor!=null && cursor.moveToFirst()) {
                int n=cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME),s=cursor.getColumnIndex(OpenableColumns.SIZE);
                if(n>=0 && !cursor.isNull(n)) name=cursor.getString(n);
                if(s>=0 && !cursor.isNull(s)) total=Math.max(0,cursor.getLong(s));
            }
        }
        if(name.length()>512) name=name.substring(0,512);
        if(total>MAX_BOOK) throw new IOException("图书超过 128 MiB 读取上限");
        if(!cache.isDirectory() && !cache.mkdirs()) throw new IOException("无法创建图书缓存");
        File temporary=File.createTempFile("import-",".tmp",cache);
        boolean moved=false;
        try {
            MessageDigest digest=MessageDigest.getInstance("SHA-256");
            long done=0;byte[] block=new byte[65536];
            try(InputStream input=resolver.openInputStream(uri);FileOutputStream out=new FileOutputStream(temporary)) {
                if(input==null) throw new IOException("文档提供程序没有返回内容");
                while(true) {
                    if(cancelled.get()) throw new IOException("导入已取消");
                    int count=input.read(block);if(count<0) break;if(count==0) continue;
                    done+=count;if(done>MAX_BOOK) throw new IOException("图书超过 128 MiB 读取上限");
                    digest.update(block,0,count);out.write(block,0,count);progress.update(done,total);
                }
                out.getFD().sync();
            }
            if(cancelled.get()) throw new IOException("导入已取消");
            String extension;
            try(RandomAccessFile input=new RandomAccessFile(temporary,"r")) {
                if(input.length()<4) throw new IOException("文件过短，不是有效图书");
                int signature=input.readInt();
                if(signature==0x504b0304) extension=".epub";
                else {
                    if(input.length()<68) throw new IOException("当前支持 EPUB、MOBI 和 AZW3");
                    input.seek(60);byte[] magic=new byte[8];input.readFully(magic);
                    if(!Arrays.equals(magic,new byte[]{'B','O','O','K','M','O','B','I'})) throw new IOException("当前支持 EPUB、MOBI 和 AZW3");
                    extension=".mobi"; // Native content detection distinguishes MOBI6/7 and KF8.
                }
            }
            StringBuilder hex=new StringBuilder(64);for(byte b:digest.digest()) hex.append(String.format(java.util.Locale.ROOT,"%02x",b&255));
            File result=new File(cache,hex+extension);
            if(result.isFile()) { if(!temporary.delete()) throw new IOException("无法清理重复导入缓存"); }
            else if(!temporary.renameTo(result)) throw new IOException("无法完成图书缓存写入");
            moved=true;result.setLastModified(System.currentTimeMillis());
            prune(cache,result,preserve);
            return new Imported(result,name);
        } finally { if(!moved) temporary.delete(); }
    }
    private static void prune(File directory,File current,File preserve) {
        File[] files=directory.listFiles(f->f.isFile() && f.getName().matches("[0-9a-f]{64}\\.(epub|mobi)"));
        if(files==null) return;
        Arrays.sort(files,Comparator.comparingLong(File::lastModified).reversed());
        for(int i=4;i<files.length;i++) if(!files[i].equals(current) && !files[i].equals(preserve)) files[i].delete();
    }
    static File font(AssetManager assets,File directory) throws IOException {
        if(!directory.isDirectory() && !directory.mkdirs()) throw new IOException("无法创建字体目录");
        File output=new File(directory,"LXGWWenKaiLite-Regular.ttf");
        if(output.isFile() && output.length()==13872424) return output;
        File temporary=File.createTempFile("font-",".tmp",directory);
        boolean installed=false;
        try(InputStream in=assets.open("LXGWWenKaiLite-Regular.ttf");FileOutputStream out=new FileOutputStream(temporary)) {
            byte[] bytes=new byte[65536];int n;long total=0;
            while((n=in.read(bytes))>=0) { total+=n;if(total>16*1024*1024) throw new IOException("字体资源过大");out.write(bytes,0,n); }
            out.getFD().sync();
            if(total!=13872424) throw new IOException("内置字体资源不完整");
            if(!temporary.renameTo(output)) throw new IOException("无法安装内置字体");
            installed=true;return output;
        } finally { if(!installed) temporary.delete(); }
    }
}
