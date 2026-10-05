package xin.soymilk.readall;

import java.io.*;
import java.nio.channels.FileChannel;
import java.nio.channels.FileLock;
import java.nio.file.Files;
import java.nio.file.StandardCopyOption;
import java.util.*;

/** Versioned private bookshelf. Index changes never delete originals, managed books or reader state. */
final class ShelfStore {
    static final int LIMIT=1000, MAGIC=0x52534c31, MAX_INDEX_BYTES=16*1024*1024;
    private static final Object MUTEX=new Object();
    final File root,books,covers;
    private final File index;
    static final class Book {
        final String id,file,name,title,author,format,uri,alias;
        final long added,opened;
        final double percent;
        final boolean pinned;
        Book(String file,String name,String title,String author,String format,String uri,String alias,long added,long opened,double percent,boolean pinned) throws IOException {
            if(!file.matches("[0-9a-f]{64}\\.(epub|mobi)"))throw new IOException("书库文件标识无效");
            this.id=file.substring(0,64);this.file=file;this.name=clean(name,512);this.title=clean(title,512);this.author=clean(author,512);this.format=clean(format,32);this.uri=clean(uri,8192);this.alias=clean(alias,512);
            if(added<0||opened<0||!Double.isFinite(percent)||percent < -1||percent>100)throw new IOException("书库阅读状态无效");
            this.added=added;this.opened=opened;this.percent=percent;this.pinned=pinned;
        }
        String label(){return !alias.isEmpty()?alias:!title.isEmpty()?title:name;}
        String progress(){return percent<0?(opened>0?"待恢复进度":"未读"):String.format(Locale.ROOT,"%.1f%%",percent);}
        Book edit(String alias,boolean pinned,long opened,double percent)throws IOException{return new Book(file,name,title,author,format,uri,alias,added,opened,percent,pinned);}
    }
    ShelfStore(File root) throws IOException {
        this.root=root.getCanonicalFile();books=new File(this.root,"books");covers=new File(this.root,"covers");index=new File(this.root,"shelf-v1.bin");
        if(!this.root.isDirectory()&&!this.root.mkdirs())throw new IOException("无法创建书库");
        if(!books.isDirectory()&&!books.mkdirs())throw new IOException("无法创建图书目录");
        if(!covers.isDirectory()&&!covers.mkdirs())throw new IOException("无法创建封面目录");
    }
    static String clean(String value,int max)throws IOException {
        if(value==null||value.length()>max||value.indexOf('\0')>=0)throw new IOException("书库文本过长或无效");return value;
    }
    File bookFile(Book book)throws IOException{return contained(books,book.file);}
    File coverFile(Book book)throws IOException{return contained(covers,book.id+".png");}
    private static File contained(File parent,String name)throws IOException {
        File file=new File(parent,name).getCanonicalFile();if(!file.getParentFile().equals(parent.getCanonicalFile()))throw new IOException("书库路径越界");return file;
    }
    List<Book> load()throws IOException {synchronized(MUTEX){return new ArrayList<>(read().values());}}
    private LinkedHashMap<String,Book> read()throws IOException {
        LinkedHashMap<String,Book> rows=new LinkedHashMap<>();if(!index.exists())return rows;
        if(index.length()>MAX_INDEX_BYTES)throw new IOException("书库索引过大；原文件保留");
        try(DataInputStream in=new DataInputStream(new BufferedInputStream(new FileInputStream(index)))) {
            if(in.readInt()!=MAGIC)throw new IOException("无法识别书库索引；原文件保留");
            int count=in.readInt();if(count<0||count>LIMIT)throw new IOException("书库条目超限");
            for(int i=0;i<count;i++){
                Book b=new Book(in.readUTF(),in.readUTF(),in.readUTF(),in.readUTF(),in.readUTF(),in.readUTF(),in.readUTF(),in.readLong(),in.readLong(),in.readDouble(),in.readBoolean());
                if(rows.put(b.id,b)!=null)throw new IOException("书库含重复标识");
            }
            if(in.read()!=-1)throw new IOException("书库索引包含未知数据");
        }return rows;
    }
    interface Change {void apply(LinkedHashMap<String,Book> rows)throws IOException;}
    private void change(Change change)throws IOException {
        synchronized(MUTEX){
            try(RandomAccessFile lockFile=new RandomAccessFile(new File(root,"shelf.lock"),"rw");FileChannel channel=lockFile.getChannel();FileLock lock=channel.lock()){
                if(!lock.isValid())throw new IOException("书库忙碌");LinkedHashMap<String,Book> rows=read();change.apply(rows);
                if(rows.size()>LIMIT)throw new IOException("书库最多保存 1000 本，请先移除不需要的记录");
                File tmp=File.createTempFile("shelf-",".tmp",root);
                try {
                    try(FileOutputStream file=new FileOutputStream(tmp);DataOutputStream out=new DataOutputStream(new BufferedOutputStream(file))){
                        out.writeInt(MAGIC);out.writeInt(rows.size());
                        for(Book b:rows.values()){out.writeUTF(b.file);out.writeUTF(b.name);out.writeUTF(b.title);out.writeUTF(b.author);out.writeUTF(b.format);out.writeUTF(b.uri);out.writeUTF(b.alias);out.writeLong(b.added);out.writeLong(b.opened);out.writeDouble(b.percent);out.writeBoolean(b.pinned);}
                        out.flush();file.getFD().sync();
                    }
                    if(tmp.length()>MAX_INDEX_BYTES)throw new IOException("书库索引超过 16 MiB 上限；原记录保留");
                    Files.move(tmp.toPath(),index.toPath(),StandardCopyOption.ATOMIC_MOVE,StandardCopyOption.REPLACE_EXISTING);
                }finally{Files.deleteIfExists(tmp.toPath());}
            }
        }
    }
    void add(Book imported)throws IOException {change(rows->{Book old=rows.get(imported.id);rows.put(imported.id,new Book(imported.file,imported.name,imported.title,imported.author,imported.format,imported.uri,old==null?"":old.alias,old==null?imported.added:old.added,old==null?0:old.opened,old==null?-1:old.percent,old!=null&&old.pinned));});}
    void remove(Collection<String> ids)throws IOException {change(rows->{for(String id:ids)rows.remove(id);});}
    void rename(String id,String title)throws IOException {String alias=clean(title.trim(),512);change(rows->{Book b=rows.get(id);if(b!=null)rows.put(id,b.edit(alias,b.pinned,b.opened,b.percent));});}
    void pin(String id,boolean value)throws IOException {change(rows->{Book b=rows.get(id);if(b!=null)rows.put(id,b.edit(b.alias,value,b.opened,b.percent));});}
    /** Legacy last-read entries are resumable even before their precise locator is opened. */
    void opened(String id,long now)throws IOException {change(rows->{Book b=rows.get(id);if(b!=null)rows.put(id,b.edit(b.alias,b.pinned,Math.max(b.opened,now),b.percent));});}
    void progress(String id,double percent,long now)throws IOException {change(rows->{Book b=rows.get(id);if(b!=null)rows.put(id,b.edit(b.alias,b.pinned,Math.max(b.opened,now),Math.max(0,Math.min(100,percent))));});}
    static List<Book> select(List<Book> all,String query,String sort){
        String q=query.trim().toLowerCase(Locale.ROOT);List<Book> visible=new ArrayList<>();
        for(Book b:all)if(q.isEmpty()||(b.label()+" "+b.author+" "+b.name+" "+b.format).toLowerCase(Locale.ROOT).contains(q))visible.add(b);
        Comparator<Book> order="title".equals(sort)?Comparator.comparing(Book::label,String.CASE_INSENSITIVE_ORDER):"added".equals(sort)?Comparator.comparingLong((Book b)->b.added).reversed():Comparator.comparingLong((Book b)->Math.max(b.opened,b.added)).reversed();
        visible.sort(Comparator.comparing((Book b)->!b.pinned).thenComparing(order).thenComparing(b->b.id));return visible;
    }
}
