package xin.soymilk.readall;

import android.app.Activity;
import android.content.res.ColorStateList;
import android.os.Build;
import android.os.SystemClock;
import android.view.accessibility.AccessibilityManager;
import android.text.Editable;
import android.text.TextWatcher;
import android.view.Gravity;
import android.view.View;
import android.view.inputmethod.InputMethodManager;
import android.widget.*;
import java.util.*;

/** Minimal native bookshelf chrome; list/grid, search and management keep their existing contracts. */
final class ShelfHome extends LinearLayout {
    interface Callbacks extends ShelfCanvas.Listener {void add();void theme();void mode(int mode);void sort(String sort);void cancelImport();}
    final Button themeButton;
    final ShelfCanvas canvas;
    private final TextView title,count,message,resumeLabel,resumeTitle,resumePercent;
    private final EditText search;
    private final ImageView searchIcon,resumeIcon,resumeArrow;
    private final Button add,sortButton,cancel,clear,dismissNotice;
    private final ShelfNotice notice=new ShelfNotice();
    private final Runnable expireNotice=()->{notice.expire(SystemClock.uptimeMillis());renderNotice();};
    private final LinearLayout searchBox,segments,continueCard,footer;
    private final ProgressBar importProgress;
    private final Button[] modes=new Button[2];
    private NativeReader.Appearance colors;
    private List<ShelfStore.Book> books=Collections.emptyList();
    private int mode;
    private String sort;
    private boolean busy,compact;
    ShelfHome(Activity activity,ShelfCoverCache covers,NativeReader.Appearance colors,int mode,String sort,Callbacks callbacks){
        super(activity);this.colors=colors;this.mode=ShelfGeometry.mode(mode);this.sort=sort;setOrientation(VERTICAL);setFocusableInTouchMode(true);
        canvas=new ShelfCanvas(activity,covers,colors,callbacks);
        LinearLayout header=row();header.setPadding(dp(20),dp(16),dp(20),dp(12));
        LinearLayout heading=column();title=text("书库",30,true);count=text("正在读取…",12,false);ShelfStyle.singleLine(count);heading.addView(title);LayoutParams cp=new LayoutParams(-1,-2);cp.topMargin=dp(6);heading.addView(count,cp);header.addView(heading,new LayoutParams(0,-2,1));
        themeButton=ShelfStyle.button(activity,"",callbacks::theme);header.addView(themeButton,new LayoutParams(dp(48),dp(48)));
        add=ShelfStyle.button(activity,"导入",callbacks::add);LayoutParams ap=new LayoutParams(dp(88),dp(48));ap.leftMargin=dp(8);header.addView(add,ap);addView(header);

        searchBox=row();searchBox.setPadding(dp(14),0,0,0);searchIcon=new ImageView(activity);searchBox.addView(searchIcon,new LayoutParams(dp(20),dp(20)));
        search=new EditText(activity);search.setTextSize(15);search.setTypeface(ShelfStyle.REGULAR);search.setSingleLine(true);search.setHint("搜索书名、作者或格式");search.setIncludeFontPadding(false);search.setPadding(dp(10),0,0,0);search.setBackground(null);search.setImeOptions(android.view.inputmethod.EditorInfo.IME_ACTION_SEARCH);search.setContentDescription("搜索书库");search.setFilters(new android.text.InputFilter[]{new android.text.InputFilter.LengthFilter(512)});searchBox.addView(search,new LayoutParams(0,dp(52),1));
        clear=ShelfStyle.button(activity,"",()->search.setText(""));clear.setContentDescription("清空搜索");searchBox.addView(clear,new LayoutParams(dp(48),dp(52)));clear.setVisibility(INVISIBLE);addView(searchBox,outer(52,0,12));
        search.setOnEditorActionListener((v,action,event)->{clearSearch();return true;});
        search.setOnFocusChangeListener((v,focused)->searchBackground());

        LinearLayout controls=row();segments=row();segments.setPadding(dp(3),dp(3),dp(3),dp(3));String[] labels={"列表","封面"};
        for(int i=0;i<modes.length;i++){final int next=i;modes[i]=ShelfStyle.button(activity,labels[i],()->{String anchor=canvas.focusId();this.mode=next;callbacks.mode(next);update(anchor);});segments.addView(modes[i],new LayoutParams(0,dp(48),1));}
        controls.addView(segments,new LayoutParams(0,dp(54),1));View space=new View(activity);controls.addView(space,new LayoutParams(dp(12),1));
        sortButton=ShelfStyle.button(activity,"最近",()->{this.sort="recent".equals(this.sort)?"title":"title".equals(this.sort)?"added":"recent";callbacks.sort(this.sort);update(null);});controls.addView(sortButton,new LayoutParams(dp(88),dp(48)));addView(controls,outer(54,0,12));

        continueCard=row();continueCard.setPadding(dp(16),dp(12),dp(14),dp(12));continueCard.setMinimumHeight(dp(80));continueCard.setFocusable(true);continueCard.setOnClickListener(v->{ShelfStore.Book latest=last();if(latest!=null)callbacks.open(latest);});
        resumeIcon=new ImageView(activity);continueCard.addView(resumeIcon,new LayoutParams(dp(24),dp(28)));LinearLayout details=column();resumeLabel=text("继续阅读",12,false);resumeTitle=text("",16,true);ShelfStyle.singleLine(resumeTitle);details.addView(resumeLabel);LayoutParams rt=new LayoutParams(-1,-2);rt.topMargin=dp(6);details.addView(resumeTitle,rt);LayoutParams dl=new LayoutParams(0,-2,1);dl.leftMargin=dp(12);dl.rightMargin=dp(8);continueCard.addView(details,dl);
        resumePercent=text("",12,false);continueCard.addView(resumePercent,new LayoutParams(-2,-2));resumeArrow=new ImageView(activity);LayoutParams ar=new LayoutParams(dp(18),dp(18));ar.leftMargin=dp(8);continueCard.addView(resumeArrow,ar);addView(continueCard,outer(-2,0,12));continueCard.setVisibility(GONE);
        addView(canvas,new LayoutParams(-1,0,1));

        footer=row();footer.setPadding(dp(12),0,dp(4),0);footer.setMinimumHeight(dp(48));importProgress=new ProgressBar(activity,null,android.R.attr.progressBarStyleSmall);footer.addView(importProgress,new LayoutParams(dp(18),dp(18)));importProgress.setVisibility(GONE);
        message=text("",12,false);message.setMaxLines(3);message.setAccessibilityLiveRegion(ACCESSIBILITY_LIVE_REGION_POLITE);LayoutParams mp=new LayoutParams(0,-2,1);mp.leftMargin=dp(8);mp.topMargin=dp(8);mp.bottomMargin=dp(8);footer.addView(message,mp);
        cancel=ShelfStyle.button(activity,"取消",callbacks::cancelImport);footer.addView(cancel,new LayoutParams(dp(64),dp(48)));cancel.setVisibility(GONE);
        dismissNotice=ShelfStyle.button(activity,"",this::clearNotice);dismissNotice.setContentDescription("关闭提示");footer.addView(dismissNotice,new LayoutParams(dp(48),dp(48)));dismissNotice.setVisibility(GONE);addView(footer,outer(-2,4,8));footer.setVisibility(GONE);
        search.addTextChangedListener(new TextWatcher(){public void beforeTextChanged(CharSequence s,int start,int count,int after){}public void onTextChanged(CharSequence s,int start,int before,int count){clear.setVisibility(s.length()>0?VISIBLE:INVISIBLE);update("");}public void afterTextChanged(Editable e){}});
        theme(colors);
    }
    private int dp(float value){return ShelfStyle.dp(getContext(),value);}
    private LayoutParams outer(int height,int top,int bottom){LayoutParams p=new LayoutParams(-1,height<0?height:dp(height));p.setMargins(dp(20),dp(top),dp(20),dp(bottom));return p;}
    private LinearLayout row(){LinearLayout r=new LinearLayout(getContext());r.setOrientation(HORIZONTAL);r.setGravity(Gravity.CENTER_VERTICAL);return r;}
    private LinearLayout column(){LinearLayout c=new LinearLayout(getContext());c.setOrientation(VERTICAL);return c;}
    private TextView text(String value,int size,boolean medium){return ShelfStyle.text(getContext(),value,size,colors.ink,medium);}
    void books(List<ShelfStore.Book> rows,String anchor){books=new ArrayList<>(rows);update(anchor);}
    private ShelfStore.Book last(){ShelfStore.Book latest=null;for(ShelfStore.Book b:books)if(b.opened>0&&(latest==null||b.opened>latest.opened))latest=b;return latest;}
    private void update(String anchor){
        List<ShelfStore.Book> visible=ShelfStore.select(books,search.getText().toString(),sort);count.setText(books.size()+" 本图书"+(visible.size()!=books.size()?" · 找到 "+visible.size()+" 本":" · 本地书库"));
        canvas.empty(!search.getText().toString().trim().isEmpty());canvas.data(visible,mode,anchor);updateContinue();sortButton.setText("title".equals(sort)?"书名":"added".equals(sort)?"导入":"最近");sortButton.setContentDescription("排序："+("title".equals(sort)?"书名":"added".equals(sort)?"导入时间":"最近阅读")+"，点击切换");styleTabs();
    }
    private void updateContinue(){ShelfStore.Book latest=last();continueCard.setVisibility(latest==null||compact?GONE:VISIBLE);if(latest!=null){resumeTitle.setText(latest.label());resumePercent.setText(latest.progress());continueCard.setContentDescription("继续阅读，"+latest.label()+"，"+latest.progress());}}
    @Override protected void onSizeChanged(int w,int h,int oldw,int oldh){super.onSizeChanged(w,h,oldw,oldh);boolean small=h<dp(440);if(small!=compact){compact=small;count.setVisibility(small?GONE:VISIBLE);updateContinue();}styleTabs();}
    void busy(boolean value,String status){busy=value;add.setEnabled(!value);continueCard.setEnabled(!value);continueCard.setAlpha(value?.55f:1f);cancel.setVisibility(value?VISIBLE:GONE);importProgress.setVisibility(value?VISIBLE:GONE);message(status);}
    boolean busy(){return busy;}
    void message(String value){
        int timeout=ShelfNotice.TIMEOUT_MS;AccessibilityManager access=(AccessibilityManager)getContext().getSystemService(Activity.ACCESSIBILITY_SERVICE);
        if(Build.VERSION.SDK_INT>=29&&access!=null)timeout=access.getRecommendedTimeoutMillis(timeout,AccessibilityManager.FLAG_CONTENT_TEXT|AccessibilityManager.FLAG_CONTENT_CONTROLS);
        notice.update(value,busy,SystemClock.uptimeMillis(),timeout);if(!isShown())notice.dismiss();renderNotice();
    }
    private void renderNotice(){
        removeCallbacks(expireNotice);notice.expire(SystemClock.uptimeMillis());message.setText(notice.text());footer.setVisibility(notice.visible()?VISIBLE:GONE);dismissNotice.setVisibility(notice.visible()&&!notice.working()?VISIBLE:GONE);
        long delay=notice.remaining(SystemClock.uptimeMillis());if(isShown()&&delay>0)postDelayed(expireNotice,delay);
    }
    void clearNotice(){notice.dismiss();renderNotice();}
    @Override protected void onVisibilityChanged(View changed,int visibility){super.onVisibilityChanged(changed,visibility);if(footer!=null&&dismissNotice!=null){if(!isShown())notice.dismiss();renderNotice();}}
    @Override protected void onWindowVisibilityChanged(int visibility){super.onWindowVisibilityChanged(visibility);if(footer!=null&&dismissNotice!=null){if(visibility!=VISIBLE)notice.dismiss();renderNotice();}}
    @Override protected void onAttachedToWindow(){super.onAttachedToWindow();renderNotice();}
    @Override protected void onDetachedFromWindow(){removeCallbacks(expireNotice);notice.dismiss();super.onDetachedFromWindow();}
    void clearSearch(){search.clearFocus();((InputMethodManager)getContext().getSystemService(Activity.INPUT_METHOD_SERVICE)).hideSoftInputFromWindow(search.getWindowToken(),0);}
    void theme(NativeReader.Appearance p){
        colors=p;setBackgroundColor(p.canvas);title.setTextColor(p.ink);count.setTextColor(p.muted);message.setTextColor(p.muted);resumeLabel.setTextColor(p.muted);resumeTitle.setTextColor(p.ink);resumePercent.setTextColor(p.accent);
        search.setTextColor(p.ink);search.setHintTextColor(p.muted);searchBackground();searchIcon.setImageDrawable(new ShelfIcon(ShelfIcon.SEARCH,p.muted,dp(20)));canvas.theme(p);
        for(Button b:new Button[]{themeButton,sortButton,cancel,clear,dismissNotice})ShelfStyle.buttonTheme(b,p,false);ShelfStyle.buttonTheme(add,p,true);
        themeButton.setContentDescription(p.dark()?"切换到亮色主题":"切换到暗色主题");themeButton.setTooltipText(themeButton.getContentDescription());ShelfStyle.icon(themeButton,p.dark()?ShelfIcon.SUN:ShelfIcon.MOON,p.ink,true);ShelfStyle.icon(add,ShelfIcon.ADD,p.onAccent,false);ShelfStyle.icon(sortButton,ShelfIcon.SORT,p.muted,false);ShelfStyle.icon(clear,ShelfIcon.CLOSE,p.muted,true);ShelfStyle.icon(dismissNotice,ShelfIcon.CLOSE,p.muted,true);
        continueCard.setBackground(ShelfStyle.touch(getContext(),p.panel,p.accent,18));resumeIcon.setImageDrawable(new ShelfIcon(ShelfIcon.BOOK,p.accent,dp(24)));resumeArrow.setImageDrawable(new ShelfIcon(ShelfIcon.ARROW,p.muted,dp(18)));footer.setBackground(ShelfStyle.shape(getContext(),p.panel,12));importProgress.setIndeterminateTintList(ColorStateList.valueOf(p.accent));styleTabs();
    }
    private void searchBackground(){android.graphics.drawable.GradientDrawable background=ShelfStyle.shape(getContext(),colors.panel,16);if(search.hasFocus())background.setStroke(dp(1),colors.accent);searchBox.setBackground(background);}
    private void styleTabs(){segments.setBackground(ShelfStyle.shape(getContext(),colors.panel,14));for(int i=0;i<modes.length;i++){boolean selected=i==mode;modes[i].setSelected(selected);modes[i].setTextColor(selected?colors.ink:colors.muted);modes[i].setBackgroundTintList(null);modes[i].setBackground(ShelfStyle.touch(getContext(),selected?colors.selected:colors.panel,colors.accent,11));if((getWidth()==0||getWidth()>=dp(360))&&getResources().getConfiguration().fontScale<=1.2f)ShelfStyle.icon(modes[i],i==0?ShelfIcon.LIST:ShelfIcon.GRID,selected?colors.accent:colors.muted,false);else{modes[i].setCompoundDrawables(null,null,null,null);modes[i].setCompoundDrawablePadding(0);}modes[i].setContentDescription((i==0?"列表模式":"封面网格模式")+(selected?"，已选择":""));}}
}
