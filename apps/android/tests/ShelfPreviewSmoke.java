package xin.soymilk.readall;

import java.io.*;
import java.nio.ByteBuffer;
import java.nio.file.Files;
import java.security.MessageDigest;
import java.util.Arrays;
import java.util.zip.*;
import java.awt.image.BufferedImage;
import javax.imageio.ImageIO;

/** Real JNI preview of original synthetic EPUB 2/3 books, without opening a reader or font. */
public final class ShelfPreviewSmoke {
    private static void check(boolean yes,String why){if(!yes)throw new AssertionError(why);}
    private static void add(ZipOutputStream zip,String name,byte[] data,boolean stored)throws Exception {ZipEntry entry=new ZipEntry(name);if(stored){CRC32 crc=new CRC32();crc.update(data);entry.setMethod(ZipEntry.STORED);entry.setSize(data.length);entry.setCompressedSize(data.length);entry.setCrc(crc.getValue());}zip.putNextEntry(entry);zip.write(data);zip.closeEntry();}
    private static byte[] utf8(String text){return text.getBytes(java.nio.charset.StandardCharsets.UTF_8);}
    private static File fixture(File root,String name,boolean epub3,byte[] image)throws Exception {
        File book=new File(root,name);
        try(ZipOutputStream zip=new ZipOutputStream(new FileOutputStream(book))){
            add(zip,"mimetype",utf8("application/epub+zip"),true);
            add(zip,"META-INF/container.xml",utf8("<container><rootfiles><rootfile full-path='OPS/book.opf' media-type='application/oebps-package+xml'/></rootfiles></container>"),false);
            add(zip,"OPS/book.opf",utf8("<package xmlns:dc='http://purl.org/dc/elements/1.1/'><metadata><dc:title>书库预览😀</dc:title><dc:creator>测试作者</dc:creator>"+(epub3?"":"<meta name='cover' content='front'/>")+"</metadata><manifest><item id='chapter' href='chapter.xhtml' media-type='application/xhtml+xml'/><item id='front' href='front.png' media-type='image/png'"+(epub3?" properties='cover-image'":"")+"/></manifest><spine><itemref idref='chapter'/></spine></package>"),false);
            add(zip,"OPS/chapter.xhtml",utf8("<html><body>正文没有参与封面排版。</body></html>"),false);add(zip,"OPS/front.png",image,false);
        }return book;
    }
    public static void main(String[] args)throws Exception {
        File root=new File(args[0]);BufferedImage image=new BufferedImage(800,1000,BufferedImage.TYPE_INT_RGB);int[] row=new int[800];Arrays.fill(row,0x0a5ab4);for(int y=0;y<1000;y++)image.setRGB(0,y,800,1,row,0,800);ByteArrayOutputStream out=new ByteArrayOutputStream();ImageIO.write(image,"png",out);
        ByteBuffer pixels=ByteBuffer.allocateDirect(NativeReader.PREVIEW_BYTES);
        for(boolean epub3:new boolean[]{false,true}){
            File book=fixture(root,"shelf-preview-"+epub3+".epub",epub3,out.toByteArray());byte[] before=MessageDigest.getInstance("SHA-256").digest(Files.readAllBytes(book.toPath()));
            NativeReader.Preview p=NativeReader.preview(book.getAbsolutePath(),pixels);check(p.title.equals("书库预览😀")&&p.author.equals("测试作者")&&p.format.equals("EPUB"),"metadata not preserved");check(p.width==384&&p.height==480,"bounded cover ratio");check((pixels.get(0)&255)==10&&(pixels.get(1)&255)==90&&(pixels.get(2)&255)==180&&(pixels.get(3)&255)==255,"cover bytes/order");check(Arrays.equals(before,MessageDigest.getInstance("SHA-256").digest(Files.readAllBytes(book.toPath()))),"preview edited the book");
        }
        File bad=fixture(root,"shelf-preview-no-image.epub",false,utf8("broken image"));check(NativeReader.preview(bad.getAbsolutePath(),pixels).width==0,"broken cover should keep metadata");
        boolean rejected=false;try{NativeReader.preview(bad.getAbsolutePath(),pixels.asReadOnlyBuffer());}catch(IllegalArgumentException e){rejected=true;}check(rejected,"read-only buffer accepted");
        java.lang.reflect.Method nativeCall=NativeReader.class.getDeclaredMethod("nativePreview",String.class,ByteBuffer.class);nativeCall.setAccessible(true);rejected=false;try{nativeCall.invoke(null,bad.getAbsolutePath(),ByteBuffer.allocateDirect(4));}catch(java.lang.reflect.InvocationTargetException e){rejected=e.getCause() instanceof IllegalStateException;}check(rejected,"native undersized buffer accepted");
        pixels.put(0,(byte)77);rejected=false;try{NativeReader.preview("relative.epub",pixels);}catch(IllegalStateException e){rejected=true;}check(rejected&&pixels.get(0)==77,"bad path modified pixels");
        System.out.println("PASS real JNI shelf preview: EPUB2/EPUB3 cover declarations, metadata/Unicode, bounded opaque thumbnails, missing covers, safe buffers and unchanged source files");
    }
}
