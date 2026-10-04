package xin.soymilk.readall;

import android.app.Activity;
import android.app.AlertDialog;
import android.content.Intent;
import android.content.SharedPreferences;
import android.graphics.Bitmap;
import android.graphics.Insets;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.text.TextUtils;
import android.view.View;
import android.view.WindowInsets;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.ProgressBar;
import android.widget.TextView;
import android.widget.Toast;
import java.io.File;
import java.nio.ByteBuffer;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

/** SDK-only native Android shell. No WebView, all-files permission or external tools. */
public final class MainActivity extends Activity implements ReaderView.Listener {
    private static final int PICK_BOOK=41;
    private final Handler ui=new Handler(Looper.getMainLooper());
    private final ExecutorService io=Executors.newSingleThreadExecutor();
    private volatile int epoch;
    private volatile boolean importing;
    private volatile long imported,total;
    private boolean resumed,destroyed,copyPending,remembered,waitingContents;
    private long shownSerial,contentsAfter;
    private NativeReader reader;
    private NativeReader.State lastState;
    private File currentFile;
    private String currentName="ReadAll",currentUri="",lastNotice="";
    private TextView title,status,position;
    private ProgressBar progress;
    private ReaderView page;
    private Button previous,next,contents,bookmark,smaller,larger,theme;
    private final Runnable poller=this::poll;
    private final Runnable resizeTask=this::resizeNative;

    @Override public void onCreate(Bundle saved) {
        super.onCreate(saved);
        LinearLayout root=new LinearLayout(this);root.setOrientation(LinearLayout.VERTICAL);root.setBackgroundColor(0xfff5f6f8);
        int padding=dp(10);root.setPadding(padding,padding,padding,padding);
        if(Build.VERSION.SDK_INT>=30) {
            getWindow().setDecorFitsSystemWindows(false);
            root.setOnApplyWindowInsetsListener((view,insets)-> {
                Insets bars=insets.getInsets(WindowInsets.Type.systemBars()|WindowInsets.Type.displayCutout());
                view.setPadding(padding+bars.left,padding+bars.top,padding+bars.right,padding+bars.bottom);return insets;
            });
        }
        title=text(18);title.setText("ReadAll · Android 开发预览");title.setMaxLines(1);title.setEllipsize(TextUtils.TruncateAt.END);root.addView(title);
        LinearLayout top=row(root);
        button(top,"打开",()->pick());button(top,"继续",this::openLast);
        smaller=button(top,"字号−",()->send(NativeReader.SMALLER));larger=button(top,"字号+",()->send(NativeReader.LARGER));theme=button(top,"主题",()->send(NativeReader.THEME));
        status=text(12);status.setText("选择 EPUB、MOBI 或 AZW3。点击正文不会翻页。");status.setMaxLines(3);root.addView(status);
        progress=new ProgressBar(this,null,android.R.attr.progressBarStyleHorizontal);progress.setMax(1000);progress.setVisibility(View.GONE);root.addView(progress,new LinearLayout.LayoutParams(-1,dp(4)));
        page=new ReaderView(this,this);root.addView(page,new LinearLayout.LayoutParams(-1,0,1));
        position=text(12);position.setGravity(android.view.Gravity.CENTER);root.addView(position);
        LinearLayout bottom=row(root);
        button(bottom,"返回",this::returnHome);previous=button(bottom,"上一页",()->send(NativeReader.PREVIOUS));contents=button(bottom,"目录",this::requestContents);bookmark=button(bottom,"书签",()->send(NativeReader.BOOKMARK));next=button(bottom,"下一页",()->send(NativeReader.NEXT));
        setContentView(root);enableReader(false);
        if(Build.VERSION.SDK_INT>=33) getOnBackInvokedDispatcher().registerOnBackInvokedCallback(android.window.OnBackInvokedDispatcher.PRIORITY_DEFAULT,this::goBack);
        if(saved!=null && saved.getBoolean("reading",false)) page.post(this::openLast);
    }
    private int dp(int n) { return Math.round(n*getResources().getDisplayMetrics().density); }
    private TextView text(int sp) { TextView view=new TextView(this);view.setTextSize(sp);view.setTextColor(0xff27364b);view.setPadding(0,dp(3),0,dp(3));return view; }
    private LinearLayout row(LinearLayout parent) { LinearLayout row=new LinearLayout(this);row.setOrientation(LinearLayout.HORIZONTAL);parent.addView(row,new LinearLayout.LayoutParams(-1,dp(48)));return row; }
    private Button button(LinearLayout row,String text,Runnable action) {
        Button button=new Button(this);button.setText(text);button.setAllCaps(false);button.setTextSize(12);button.setMinWidth(0);button.setMinimumWidth(0);button.setPadding(0,0,0,0);button.setContentDescription(text);
        row.addView(button,new LinearLayout.LayoutParams(0,-1,1));button.setOnClickListener(v->action.run());return button;
    }
    private void enableReader(boolean enabled) { for(Button b:new Button[]{previous,next,contents,bookmark,smaller,larger,theme}) if(b!=null) b.setEnabled(enabled); }
    private void pick() {
        Intent intent=new Intent(Intent.ACTION_OPEN_DOCUMENT);intent.addCategory(Intent.CATEGORY_OPENABLE);intent.setType("*/*");
        intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION|Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION);
        startActivityForResult(intent,PICK_BOOK);
    }
    @Override protected void onActivityResult(int request,int result,Intent data) {
        super.onActivityResult(request,result,data);
        if(request!=PICK_BOOK || result!=RESULT_OK || data==null || data.getData()==null) return;
        Uri uri=data.getData();
        try { getContentResolver().takePersistableUriPermission(uri,data.getFlags()&Intent.FLAG_GRANT_READ_URI_PERMISSION); } catch(SecurityException ignored) { /* Some providers grant only this session. */ }
        importBook(uri);
    }
    private void importBook(Uri uri) {
        File preserve=currentFile;closeReader();int token=++epoch;importing=true;imported=0;total=0;currentUri=uri.toString();
        status.setText("正在从系统文档提供程序读取图书…");enableReader(false);schedule();
        io.execute(()-> {
            try {
                BookFiles.Imported book=BookFiles.read(getContentResolver(),uri,new File(getCacheDir(),"books"),preserve,(done,all)->{if(token==epoch) { imported=done;total=all; }},()->token!=epoch||destroyed||Thread.currentThread().isInterrupted());
                File font=BookFiles.font(getAssets(),new File(getFilesDir(),"fonts"));
                ui.post(()-> { if(token!=epoch||destroyed) return;importing=false;startReader(book.file,book.name,font,token); });
            } catch(Exception error) { ui.post(()-> { if(token==epoch&&!destroyed) { importing=false;showError(error); } }); }
        });
    }
    private void openLast() {
        if(importing) return;
        SharedPreferences saved=getSharedPreferences("library",MODE_PRIVATE);
        String path=saved.getString("book","");String uri=saved.getString("uri","");
        File file=new File(path);
        try {
            if(path.isEmpty() || !file.isFile() || !file.getCanonicalPath().startsWith(new File(getCacheDir(),"books").getCanonicalPath()+File.separator)) {
                if(!uri.isEmpty()) { importBook(Uri.parse(uri));return; }
                pick();return;
            }
        } catch(Exception error) { showError(error);return; }
        closeReader();int token=++epoch;currentUri=uri;importing=true;imported=0;total=0;schedule();
        io.execute(()-> {
            try {
                File font=BookFiles.font(getAssets(),new File(getFilesDir(),"fonts"));
                ui.post(()-> { if(token!=epoch||destroyed)return;importing=false;startReader(file,saved.getString("name","图书"),font,token); });
            } catch(Exception error) { ui.post(()->{if(token==epoch&&!destroyed){importing=false;showError(error);}}); }
        });
    }
    private int[] viewport() {
        int w=Math.max(256,page.getWidth()),h=Math.max(256,page.getHeight());
        double scale=Math.min(1.0,Math.sqrt(1600000.0/((double)w*h)));
        return new int[]{Math.max(256,(int)Math.round(w*scale)),Math.max(256,(int)Math.round(h*scale))};
    }
    private void startReader(File file,String name,File font,int token) {
        if(token!=epoch || destroyed) return;
        if(page.getWidth()==0 || page.getHeight()==0) { page.post(()->startReader(file,name,font,token));return; }
        try {
            int[] v=viewport();float scale=(float)v[0]/page.getWidth()*getResources().getDisplayMetrics().scaledDensity;
            reader=new NativeReader(file.getAbsolutePath(),font.getAbsolutePath(),new File(getFilesDir(),"reader-state").getAbsolutePath(),v[0],v[1],Math.max(12,Math.min(64,Math.round(18*scale))),Math.max(8,Math.min(80,Math.round(12*scale))));
            currentFile=file;currentName=name;title.setText(name);shownSerial=0;copyPending=false;remembered=false;waitingContents=false;lastNotice="";lastState=null;
            status.setText("正在准备正文…");schedule();
        } catch(Exception|LinkageError error) { showError(error); }
    }
    @Override public void viewport(int width,int height) { ui.removeCallbacks(resizeTask);ui.postDelayed(resizeTask,120); }
    private void resizeNative() {
        if(reader==null) return;
        int[] viewport=viewport();try { reader.command(NativeReader.RESIZE,viewport[0],viewport[1]); } catch(Exception error) { showError(error); }
    }
    @Override public void turn(boolean next) { if(lastState!=null&&!lastState.busy()&&!lastState.closed()) send(next?NativeReader.NEXT:NativeReader.PREVIOUS); }
    private void send(int code) { if(reader==null)return;try{reader.command(code);schedule();}catch(Exception error){showError(error);} }
    private void requestContents() {
        if(reader==null)return;contentsAfter=lastState==null?0:lastState.revision;waitingContents=true;send(NativeReader.CONTENTS);
    }
    private void showContents(NativeReader owner) {
        waitingContents=false;
        try {
            NativeReader.Item[] items=owner.contents();String[] names=new String[items.length];
            if(items.length==0) { Toast.makeText(this,"当前图书没有可用目录",Toast.LENGTH_SHORT).show();return; }
            for(int i=0;i<items.length;i++) { StringBuilder label=new StringBuilder();for(int n=0;n<Math.min(8,items[i].depth);n++)label.append("　");names[i]=label+items[i].title; }
            new AlertDialog.Builder(this).setTitle("章节目录").setItems(names,(dialog,which)-> {
                if(reader!=owner)return;
                try { owner.command(NativeReader.JUMP,items[which].spine,items[which].offset);schedule(); }catch(Exception error){showError(error);}
            }).setNegativeButton("取消",null).show();
        }catch(Exception error){showError(error);}
    }
    private void schedule() { ui.removeCallbacks(poller);if(resumed&&!destroyed)ui.post(poller); }
    private void poll() {
        if(!resumed||destroyed)return;
        if(importing) {
            showProgress("读取图书",imported,total);ui.postDelayed(poller,100);return;
        }
        NativeReader owner=reader;
        if(owner==null) {progress.setVisibility(View.GONE);return;}
        try {
            NativeReader.State state=owner.state();lastState=state;enableReader(!state.busy()&&!state.closed()&&state.serial>0);
            if(state.busy()) showProgress(state.phase,state.done,state.total);
            else { progress.setVisibility(View.GONE);status.setText(state.notice.isEmpty()?"左右滑动或点击按钮翻页 · 返回关闭图书":state.notice); }
            if(state.serial>0) {
                title.setText(state.title.isEmpty()?currentName:state.title);position.setText(state.position+" · "+state.percent+"%");
                if(!remembered) { remembered=true;getSharedPreferences("library",MODE_PRIVATE).edit().putString("book",currentFile.getAbsolutePath()).putString("name",currentName).putString("uri",currentUri).apply(); }
                if(state.serial!=shownSerial&&!copyPending) copyFrame(owner,state,epoch);
            }
            if(!state.notice.isEmpty()&&!state.notice.equals(lastNotice)) {lastNotice=state.notice;Toast.makeText(this,state.notice,Toast.LENGTH_LONG).show();}
            if(waitingContents&&!state.busy()&&!state.closed()&&state.revision>contentsAfter)showContents(owner);
            if(state.closed()) {status.setText(state.notice.isEmpty()?"阅读已停止":state.notice);progress.setVisibility(View.GONE);return;}
            ui.postDelayed(poller,state.busy()?80:220);
        } catch(Exception|LinkageError error) { showError(error); }
    }
    private void copyFrame(NativeReader owner,NativeReader.State state,int token) {
        copyPending=true;
        io.execute(()-> {
            Bitmap bitmap=null;Throwable failure=null;
            try {
                ByteBuffer buffer=ByteBuffer.allocateDirect(state.byteLength());
                if(owner.copyPixels(state,buffer)) { buffer.position(0);bitmap=Bitmap.createBitmap(state.width,state.height,Bitmap.Config.ARGB_8888);bitmap.copyPixelsFromBuffer(buffer); }
            } catch(Exception|OutOfMemoryError error) {failure=error;}
            Bitmap image=bitmap;Throwable error=failure;
            ui.post(()-> {
                if(token!=epoch||destroyed||reader!=owner)return;
                copyPending=false;if(error!=null){showError(error);return;}
                if(image!=null){page.picture(image);shownSerial=state.serial;}
            });
        });
    }
    private void showProgress(String phase,long done,long total) {
        progress.setVisibility(View.VISIBLE);progress.setIndeterminate(total<=0);
        if(total>0) progress.setProgress((int)Math.min(1000,1000.0*done/total));
        status.setText(phase+(total>0?String.format(java.util.Locale.ROOT," · %.1f%%",100.0*Math.min(done,total)/total):"…"));
    }
    private void showError(Throwable error) {status.setText("ReadAll："+(error.getMessage()==null?error.getClass().getSimpleName():error.getMessage()));progress.setVisibility(View.GONE);enableReader(false);}
    private void closeReader() {
        ui.removeCallbacks(poller);ui.removeCallbacks(resizeTask);waitingContents=false;copyPending=false;
        NativeReader owner=reader;reader=null;lastState=null;if(owner!=null)owner.close();
    }
    private void returnHome() {++epoch;importing=false;closeReader();page.picture(null);position.setText("");title.setText("ReadAll · Android 开发预览");status.setText("选择图书开始阅读；原书和已保存的阅读记录均保留。");progress.setVisibility(View.GONE);enableReader(false);}
    private void goBack() {if(reader!=null||importing)returnHome();else finish();}
    @Override public void onBackPressed() {goBack();}
    @Override protected void onResume() {super.onResume();resumed=true;schedule();}
    @Override protected void onPause() {resumed=false;ui.removeCallbacks(poller);if(reader!=null)try{reader.command(NativeReader.SAVE);}catch(Exception ignored){}super.onPause();}
    @Override protected void onSaveInstanceState(Bundle out) {out.putBoolean("reading",reader!=null);super.onSaveInstanceState(out);}
    @Override protected void onDestroy() {destroyed=true;++epoch;closeReader();io.shutdownNow();super.onDestroy();}
}
