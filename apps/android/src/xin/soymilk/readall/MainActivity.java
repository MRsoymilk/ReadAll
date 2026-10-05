package xin.soymilk.readall;

import android.app.Activity;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.content.Intent;
import android.content.SharedPreferences;
import android.graphics.Bitmap;
import android.graphics.Insets;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.os.SystemClock;
import android.view.Choreographer;
import android.view.Gravity;
import android.view.View;
import android.view.WindowInsets;
import android.widget.Button;
import android.widget.FrameLayout;
import android.widget.LinearLayout;
import android.widget.ProgressBar;
import android.widget.TextView;
import java.io.File;
import java.nio.ByteBuffer;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.atomic.AtomicReference;

/** Native Android lifecycle/SAF services around the shared Linux reader presentation. */
public final class MainActivity extends Activity implements ReaderView.Listener {
    private static final int PICK_BOOK=41;
    private final Handler ui=new Handler(Looper.getMainLooper());
    private final ExecutorService io=Executors.newSingleThreadExecutor(),pixels=Executors.newSingleThreadExecutor();
    private final AtomicReference<Bitmap> spare=new AtomicReference<>();
    private ByteBuffer pixelBuffer; // owned only by the single pixel executor.
    private volatile int epoch;
    private volatile boolean importing,destroyed;
    private volatile long imported,total;
    private boolean resumed,frameScheduled,copyPending,remembered;
    private long shownSerial;
    private final LoadingFeedback loadingFeedback=new LoadingFeedback();
    private PageLoadingIndicator pageLoading;
    private NativeReader reader;
    private NativeReader.State lastState;
    private File currentFile;
    private String currentName="ReadAll",currentUri="";
    private ReaderView page;
    private FrameLayout root;
    private LinearLayout home,loading;
    private Button themeButton;
    private NativeReader.Appearance appearance;
    private boolean themeBusy;
    private int appearanceEpoch;
    private TextView status;
    private ProgressBar progress;
    private Choreographer choreographer;
    private final Runnable schedule=this::requestFrame;
    private final Runnable resizeTask=this::resizeNative;
    private final Choreographer.FrameCallback frames=time->{frameScheduled=false;poll();};

    @Override public void onCreate(Bundle saved){
        super.onCreate(saved);choreographer=Choreographer.getInstance();
        String savedTheme=getSharedPreferences("appearance",MODE_PRIVATE).getString("theme","light");
        try{appearance=NativeReader.appearance(savedTheme);}catch(IllegalStateException e){appearance=NativeReader.appearance("light");}
        root=new FrameLayout(this);root.setBackgroundColor(appearance.canvas);
        if(Build.VERSION.SDK_INT>=30){
            getWindow().setDecorFitsSystemWindows(false);
            root.setOnApplyWindowInsetsListener((view,insets)->{
                Insets bars=insets.getInsets(WindowInsets.Type.systemBars()|WindowInsets.Type.displayCutout());
                int bottom=Math.max(bars.bottom,insets.getInsets(WindowInsets.Type.ime()).bottom);
                view.setPadding(bars.left,bars.top,bars.right,bottom);return insets;
            });
        }
        page=new ReaderView(this,this);root.addView(page,new FrameLayout.LayoutParams(-1,-1));
        home=new LinearLayout(this);home.setOrientation(LinearLayout.VERTICAL);home.setGravity(Gravity.CENTER);home.setPadding(dp(24),dp(24),dp(24),dp(24));home.setBackgroundColor(appearance.canvas);
        TextView title=text("ReadAll",30);title.setGravity(Gravity.CENTER);home.addView(title);
        TextView hint=text("EPUB · MOBI · AZW3\n与 Linux 共用阅读界面、目录和翻页模式",14);hint.setGravity(Gravity.CENTER);home.addView(hint);
        button(home,"打开图书",this::pick);button(home,"继续阅读",this::openLast);
        themeButton=button(home,"切换到暗色",this::toggleTheme);
        root.addView(home,new FrameLayout.LayoutParams(-1,-1));
        loading=new LinearLayout(this);loading.setOrientation(LinearLayout.VERTICAL);loading.setPadding(dp(20),dp(16),dp(20),dp(16));loading.setBackgroundColor(appearance.panel);
        status=text("准备打开",14);loading.addView(status);
        progress=new ProgressBar(this,null,android.R.attr.progressBarStyleHorizontal);progress.setMax(1000);loading.addView(progress,new LinearLayout.LayoutParams(-1,dp(5)));
        LinearLayout actions=new LinearLayout(this);loading.addView(actions);button(actions,"返回 / 取消",this::returnHome);button(actions,"重试",()->{if(!currentUri.isEmpty())importBook(Uri.parse(currentUri));else openLast();});
        FrameLayout.LayoutParams lp=new FrameLayout.LayoutParams(-1,-2,Gravity.CENTER);lp.setMargins(dp(20),0,dp(20),0);root.addView(loading,lp);loading.setVisibility(View.GONE);
        pageLoading=new PageLoadingIndicator(this);
        FrameLayout.LayoutParams spinner=new FrameLayout.LayoutParams(dp(22),dp(22),Gravity.BOTTOM|Gravity.RIGHT);spinner.setMargins(0,0,dp(12),dp(10));
        // Overlay inside the root's system-bar/keyboard insets; never resize the page.
        root.addView(pageLoading,spinner);
        setContentView(root);applyAppearance(appearance);loadAppearance();
        if(Build.VERSION.SDK_INT>=33)getOnBackInvokedDispatcher().registerOnBackInvokedCallback(android.window.OnBackInvokedDispatcher.PRIORITY_DEFAULT,this::goBack);
        if(saved!=null&&saved.getBoolean("reading",false))page.post(this::openLast);
    }
    private int dp(int n){return Math.round(n*getResources().getDisplayMetrics().density);}
    private TextView text(String value,int size){TextView v=new TextView(this);v.setText(value);v.setTextSize(size);v.setTextColor(appearance.ink);v.setPadding(0,dp(8),0,dp(8));return v;}
    private Button button(LinearLayout parent,String label,Runnable action){Button b=new Button(this);b.setText(label);b.setAllCaps(false);b.setOnClickListener(v->action.run());parent.addView(b,new LinearLayout.LayoutParams(parent.getOrientation()==LinearLayout.HORIZONTAL?0:-1,dp(48),parent.getOrientation()==LinearLayout.HORIZONTAL?1:0));return b;}
    private String stateDirectory(){return new File(getFilesDir(),"reader-state").getAbsolutePath();}
    private void applyAppearance(NativeReader.Appearance value){
        appearance=value;AndroidTheme.apply(this,root,home,loading,page,progress,pageLoading,value);
        themeButton.setText(value.dark()?"切换到亮色":"切换到暗色");
        SharedPreferences mirror=getSharedPreferences("appearance",MODE_PRIVATE);
        if(!value.name.equals(mirror.getString("theme","")))mirror.edit().putString("theme",value.name).apply();
    }
    private void loadAppearance(){
        final int request=++appearanceEpoch;final String state=stateDirectory();
        io.execute(()->{try{NativeReader.Appearance value=NativeReader.loadAppearance(state);ui.post(()->{if(!destroyed&&request==appearanceEpoch&&reader==null)applyAppearance(value);});}catch(Exception e){ui.post(()->{if(!destroyed&&request==appearanceEpoch&&reader==null)showError(e);});}});
    }
    private void toggleTheme(){
        if(themeBusy||reader!=null||importing)return;
        themeBusy=true;themeButton.setEnabled(false);final int request=++appearanceEpoch;
        final String state=stateDirectory(),next=appearance.dark()?"light":"dark";
        io.execute(()->{try{NativeReader.Appearance value=NativeReader.saveTheme(state,next);ui.post(()->{if(destroyed)return;themeBusy=false;themeButton.setEnabled(true);if(request==appearanceEpoch&&reader==null)applyAppearance(value);});}catch(Exception e){ui.post(()->{if(destroyed)return;themeBusy=false;themeButton.setEnabled(true);showError(e);});}});
    }
    private void pick(){if(themeBusy)return;Intent i=new Intent(Intent.ACTION_OPEN_DOCUMENT);i.addCategory(Intent.CATEGORY_OPENABLE);i.setType("*/*");i.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION|Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION);startActivityForResult(i,PICK_BOOK);}
    @Override protected void onActivityResult(int request,int result,Intent data){
        super.onActivityResult(request,result,data);if(request!=PICK_BOOK||result!=RESULT_OK||data==null||data.getData()==null)return;
        Uri uri=data.getData();try{getContentResolver().takePersistableUriPermission(uri,data.getFlags()&Intent.FLAG_GRANT_READ_URI_PERMISSION);}catch(SecurityException ignored){}
        importBook(uri);
    }
    private void importBook(Uri uri){
        File preserve=currentFile;closeReader();int token=++epoch;importing=true;imported=total=0;currentUri=uri.toString();home.setVisibility(View.GONE);showProgress("读取图书",0,0);requestFrame();
        io.execute(()->{
            try{
                BookFiles.Imported b=BookFiles.read(getContentResolver(),uri,new File(getCacheDir(),"books"),preserve,(done,all)->{if(token==epoch){imported=done;total=all;}},()->token!=epoch||destroyed||Thread.currentThread().isInterrupted());
                File font=BookFiles.font(getAssets(),new File(getFilesDir(),"fonts"));
                ui.post(()->{if(token!=epoch||destroyed)return;importing=false;startReader(b.file,b.name,font,token);});
            }catch(Exception e){ui.post(()->{if(token==epoch&&!destroyed){importing=false;showError(e);}});}
        });
    }
    private void openLast(){
        if(importing||themeBusy)return;
        SharedPreferences saved=getSharedPreferences("library",MODE_PRIVATE);String path=saved.getString("book",""),uri=saved.getString("uri","");File file=new File(path);
        try{
            if(path.isEmpty()||!file.isFile()||!file.getCanonicalPath().startsWith(new File(getCacheDir(),"books").getCanonicalPath()+File.separator)){if(!uri.isEmpty())importBook(Uri.parse(uri));else pick();return;}
        }catch(Exception e){showError(e);return;}
        closeReader();int token=++epoch;currentUri=uri;importing=true;imported=total=0;home.setVisibility(View.GONE);showProgress("准备继续阅读",0,0);requestFrame();
        io.execute(()->{try{File font=BookFiles.font(getAssets(),new File(getFilesDir(),"fonts"));ui.post(()->{if(token!=epoch||destroyed)return;importing=false;startReader(file,saved.getString("name","图书"),font,token);});}catch(Exception e){ui.post(()->{if(token==epoch&&!destroyed){importing=false;showError(e);}});}});
    }
    private void startReader(File file,String name,File font,int token){
        if(token!=epoch||destroyed)return;if(page.getWidth()==0||page.getHeight()==0){page.post(()->startReader(file,name,font,token));return;}
        try{
            int[] v=page.viewportSize();reader=new NativeReader(file.getAbsolutePath(),font.getAbsolutePath(),new File(getFilesDir(),"reader-state").getAbsolutePath(),v[0],v[1],20,16,v[2],v[3]);
            currentFile=file;currentName=name;shownSerial=0;remembered=false;lastState=null;loadingFeedback.reset();home.setVisibility(View.GONE);showProgress("准备正文",0,0);requestFrame();
        }catch(Exception|LinkageError e){showError(e);}
    }
    @Override public void viewport(int w,int h){ui.removeCallbacks(resizeTask);ui.postDelayed(resizeTask,90);}
    private void resizeNative(){if(reader==null)return;int[] v=page.viewportSize();try{reader.viewport(v[0],v[1],v[2],v[3]);requestFrame();}catch(Exception e){showError(e);}}
    @Override public void action(int code,int a,int b){
        if(reader==null)return;
        if(code>=NativeReader.TOUCH && code<=NativeReader.TOUCH+9 && (lastState==null||lastState.serial==0||lastState.closed()) && code!=NativeReader.TOUCH+4 && code!=NativeReader.TOUCH+7 && code!=NativeReader.TOUCH+8)return;
        try{reader.command(code,a,b);requestFrame();}catch(Exception e){if(code<NativeReader.TOUCH)showError(e);}
    }
    @Override public void input(String mode,String value){if(reader==null)return;try{reader.input(mode,value);requestFrame();}catch(Exception e){showError(e);}}
    private void requestFrame(){
        ui.removeCallbacks(schedule);
        if(resumed&&!destroyed&&!frameScheduled){frameScheduled=true;choreographer.postFrameCallback(frames);}
    }
    private void poll(){
        if(!resumed||destroyed)return;
        if(importing){showProgress("读取图书",imported,total);ui.postDelayed(schedule,80);return;}
        NativeReader owner=reader;if(owner==null)return;
        try{
            NativeReader.State s=owner.state();lastState=s;
            if(s.closed()){
                if(s.notice.isEmpty()){returnHome();return;}showError(new IllegalStateException(s.notice));return;
            }
            updateLoading(s);
            if(s.serial>0){
                if(!appearance.name.equals(s.appearance.name))applyAppearance(s.appearance);
                if(!remembered){remembered=true;getSharedPreferences("library",MODE_PRIVATE).edit().putString("book",currentFile.getAbsolutePath()).putString("name",currentName).putString("uri",currentUri).apply();}
                page.state(s);
                if(s.serial!=shownSerial&&!copyPending)copyFrame(owner,s,epoch);
            }
            effects(owner);
            if(s.animating||page.touching()||copyPending)requestFrame();else ui.postDelayed(schedule,s.busy()?60:80);
        }catch(Exception|LinkageError e){showError(e);}
    }
    private void recycle(Bitmap image){if(image!=null&&!image.isRecycled())image.recycle();}
    private void release(Bitmap image){recycle(spare.getAndSet(image));}
    private void copyFrame(NativeReader owner,NativeReader.State state,int token){
        copyPending=true;
        pixels.execute(()->{
            Bitmap image=null;Throwable problem=null;
            try{
                if(pixelBuffer==null||pixelBuffer.capacity()!=state.byteLength())pixelBuffer=ByteBuffer.allocateDirect(state.byteLength());
                if(owner.copyPixels(state,pixelBuffer)){
                    image=spare.getAndSet(null);
                    if(image!=null&&(image.getWidth()!=state.width||image.getHeight()!=state.height)){recycle(image);image=null;}
                    if(image==null){image=Bitmap.createBitmap(state.width,state.height,Bitmap.Config.ARGB_8888);image.setDensity(Bitmap.DENSITY_NONE);}
                    pixelBuffer.position(0);image.copyPixelsFromBuffer(pixelBuffer);
                }
            }catch(Exception|OutOfMemoryError e){problem=e;}
            Bitmap ready=image;Throwable error=problem;
            ui.post(()->{
                if(token!=epoch||destroyed||reader!=owner){recycle(ready);return;}
                copyPending=false;
                if(error!=null){recycle(ready);showError(error);return;}
                if(ready!=null){release(page.picture(ready,state));shownSerial=state.serial;}
                requestFrame();
            });
        });
    }
    private void effects(NativeReader owner){
        String[] e=owner.effects();
        for(int n=0;n<e.length;n+=2){
            try{
                if("copy".equals(e[n])){((ClipboardManager)getSystemService(Context.CLIPBOARD_SERVICE)).setPrimaryClip(ClipData.newPlainText("ReadAll",e[n+1]));owner.hostReply(1,"");}
                else if("paste".equals(e[n])){ClipData clip=((ClipboardManager)getSystemService(Context.CLIPBOARD_SERVICE)).getPrimaryClip();CharSequence t=clip!=null&&clip.getItemCount()>0?clip.getItemAt(0).getText():null;owner.hostReply(2,t==null?"":t.toString());}
                else if("url".equals(e[n])){Uri uri=Uri.parse(e[n+1]);if(!"https".equals(uri.getScheme())&&!"http".equals(uri.getScheme()))throw new IllegalArgumentException("不支持的链接类型");startActivity(new Intent(Intent.ACTION_VIEW,uri));owner.hostReply(3,"");}
            }catch(Exception error){try{owner.hostReply(0,error.getMessage()==null?"系统服务不可用":error.getMessage());}catch(Exception ignored){}}
        }
    }
    private void updateLoading(NativeReader.State state){
        int feedback=loadingFeedback.update(shownSerial>0,state.busy(),SystemClock.uptimeMillis());
        if(feedback==LoadingFeedback.INITIAL){
            if(state.busy())showProgress(state.phase,state.done,state.total);else showProgress("显示页面",0,0);
        }else{
            // Once a book is visible, ordinary page work can ONLY use the corner spinner.
            loading.setVisibility(View.GONE);pageLoading.show(feedback==LoadingFeedback.CORNER);
        }
    }
    private void hidePageLoading(){loadingFeedback.reset();pageLoading.show(false);}
    private void showProgress(String phase,long done,long total){
        hidePageLoading();loading.setVisibility(View.VISIBLE);progress.setVisibility(View.VISIBLE);progress.setIndeterminate(total<=0);
        if(total>0)progress.setProgress((int)Math.min(1000,1000.0*done/total));
        status.setText(phase+(total>0?String.format(java.util.Locale.ROOT," · %.1f%%",100.0*Math.min(done,total)/total):"…"));
    }
    private void showError(Throwable e){hidePageLoading();loading.setVisibility(View.VISIBLE);progress.setVisibility(View.GONE);status.setText("ReadAll："+(e.getMessage()==null?e.getClass().getSimpleName():e.getMessage()));}
    private void closeReader(){
        hidePageLoading();shownSerial=0;
        page.cancelTouch();page.closeInput();ui.removeCallbacks(schedule);ui.removeCallbacks(resizeTask);choreographer.removeFrameCallback(frames);frameScheduled=false;copyPending=false;
        NativeReader owner=reader;reader=null;lastState=null;if(owner!=null)owner.close();
    }
    private void returnHome(){++epoch;importing=false;closeReader();release(page.picture(null));loading.setVisibility(View.GONE);home.setVisibility(View.VISIBLE);loadAppearance();}
    private void goBack(){
        if(importing){returnHome();return;}
        if(reader!=null){if(lastState==null||lastState.serial==0||lastState.closed())returnHome();else action(NativeReader.BACK,0,0);return;}
        finish();
    }
    @Override public void onWindowFocusChanged(boolean focused){super.onWindowFocusChanged(focused);if(focused&&appearance!=null&&root!=null)AndroidTheme.apply(this,root,home,loading,page,progress,pageLoading,appearance);}
    @Override public void onBackPressed(){goBack();}
    @Override protected void onResume(){super.onResume();resumed=true;if(reader!=null)action(NativeReader.PAUSE,0,0);requestFrame();}
    @Override protected void onPause(){resumed=false;hidePageLoading();page.cancelTouch();ui.removeCallbacks(schedule);choreographer.removeFrameCallback(frames);frameScheduled=false;if(reader!=null)try{reader.command(NativeReader.PAUSE,1,0);}catch(Exception ignored){}super.onPause();}
    @Override protected void onSaveInstanceState(Bundle state){state.putBoolean("reading",reader!=null);super.onSaveInstanceState(state);}
    @Override protected void onDestroy(){destroyed=true;++epoch;closeReader();io.shutdownNow();pixels.shutdownNow();release(null);super.onDestroy();}
}
