package xin.soymilk.readall;

import android.app.Activity;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.content.res.ColorStateList;
import android.text.Editable;
import android.text.TextWatcher;
import android.view.Gravity;
import android.view.View;
import android.widget.*;
import java.util.*;

/** Native, compact bookshelf controls around the shared-palette virtualized cover canvas. */
final class ShelfHome extends LinearLayout {
    interface Callbacks extends ShelfCanvas.Listener {void add();void theme();void mode(int mode);void sort(String sort);void cancelImport();}
    final Button themeButton;
    final ShelfCanvas canvas;
    private final TextView count,message;
    private final EditText search;
    private final Button add,sortButton,cancel,continueButton;
    private final Button[] modes=new Button[2];
    private final Callbacks callbacks;
    private NativeReader.Appearance colors;
    private List<ShelfStore.Book> books=Collections.emptyList();
    private int mode;
    private String sort;
    private boolean busy;
    ShelfHome(Activity activity,ShelfCoverCache covers,NativeReader.Appearance colors,int mode,String sort,Callbacks callbacks){
        super(activity);this.colors=colors;this.mode=ShelfGeometry.mode(mode);this.sort=sort;this.callbacks=callbacks;setOrientation(VERTICAL);canvas=new ShelfCanvas(activity,covers,colors,callbacks);
        LinearLayout header=row();header.setPadding(dp(18),dp(8),dp(14),0);TextView title=text("书库",27);title.setTypeface(Typeface.create("sans-serif-medium",Typeface.NORMAL));header.addView(title,new LayoutParams(0,dp(48),1));themeButton=button("暗色",callbacks::theme);header.addView(themeButton,new LayoutParams(dp(60),dp(40)));add=button("＋导入",callbacks::add);header.addView(add,new LayoutParams(dp(80),dp(40)));addView(header);
        count=text("正在读取书库…",12);count.setPadding(dp(20),0,dp(20),dp(7));addView(count);
        LinearLayout searchRow=row();searchRow.setPadding(dp(16),0,dp(16),dp(7));search=new EditText(activity);search.setSingleLine(true);search.setTextSize(14);search.setHint("搜索书名、作者或格式");search.setPadding(dp(13),0,dp(13),0);searchRow.addView(search,new LayoutParams(0,dp(40),1));sortButton=button("最近",()->{this.sort="recent".equals(this.sort)?"title":"title".equals(this.sort)?"added":"recent";callbacks.sort(this.sort);update(null);});searchRow.addView(sortButton,new LayoutParams(dp(68),dp(40)));addView(searchRow);
        LinearLayout tabs=row();tabs.setPadding(dp(16),0,dp(16),dp(7));String[] labels={"列表","封面"};for(int i=0;i<modes.length;i++){final int next=i;modes[i]=button(labels[i],()->{String anchor=canvas.focusId();this.mode=next;callbacks.mode(next);update(anchor);});LayoutParams lp=new LayoutParams(0,dp(38),1);if(i>0)lp.leftMargin=dp(6);tabs.addView(modes[i],lp);}addView(tabs);
        continueButton=button("继续阅读",()->{ShelfStore.Book latest=last();if(latest!=null)callbacks.open(latest);});LayoutParams resumeLp=new LayoutParams(-1,dp(36));resumeLp.setMargins(dp(16),0,dp(16),dp(4));continueButton.setMaxLines(1);continueButton.setEllipsize(android.text.TextUtils.TruncateAt.END);addView(continueButton,resumeLp);continueButton.setVisibility(GONE);
        addView(canvas,new LayoutParams(-1,0,1));
        LinearLayout footer=row();footer.setPadding(dp(16),0,dp(12),dp(5));message=text("长按图书或点击 ⋯ 管理",11);message.setMaxLines(2);footer.addView(message,new LayoutParams(0,dp(36),1));cancel=button("取消导入",callbacks::cancelImport);footer.addView(cancel,new LayoutParams(dp(86),dp(36)));cancel.setVisibility(GONE);addView(footer);
        search.addTextChangedListener(new TextWatcher(){public void beforeTextChanged(CharSequence s,int start,int count,int after){}public void onTextChanged(CharSequence s,int start,int before,int count){update("");}public void afterTextChanged(Editable e){}});
        theme(colors);
    }
    private int dp(float x){return Math.round(x*getResources().getDisplayMetrics().density);}
    private LinearLayout row(){LinearLayout row=new LinearLayout(getContext());row.setOrientation(HORIZONTAL);row.setGravity(Gravity.CENTER_VERTICAL);return row;}
    private TextView text(String s,int size){TextView t=new TextView(getContext());t.setText(s);t.setTextSize(size);t.setGravity(Gravity.CENTER_VERTICAL);return t;}
    private Button button(String title,Runnable action){Button b=new Button(getContext());b.setText(title);b.setTextSize(13);b.setAllCaps(false);b.setMinWidth(0);b.setMinimumWidth(0);b.setMinHeight(0);b.setMinimumHeight(0);b.setPadding(dp(6),0,dp(6),0);b.setOnClickListener(v->action.run());return b;}
    void books(List<ShelfStore.Book> rows,String anchor){books=new ArrayList<>(rows);update(anchor);}
    private ShelfStore.Book last(){ShelfStore.Book last=null;for(ShelfStore.Book b:books)if(b.opened>0&&(last==null||b.opened>last.opened))last=b;return last;}
    private void update(String anchor){
        List<ShelfStore.Book> visible=ShelfStore.select(books,search.getText().toString(),sort);count.setText(books.size()+" 本图书"+(visible.size()!=books.size()?" · 找到 "+visible.size()+" 本":" · EPUB / MOBI / AZW3"));
        canvas.empty(!search.getText().toString().trim().isEmpty());canvas.data(visible,mode,anchor);ShelfStore.Book last=last();continueButton.setVisibility(last==null?GONE:VISIBLE);if(last!=null)continueButton.setText("继续阅读 · "+last.label()+" · "+last.progress());
        sortButton.setText("title".equals(sort)?"书名":"added".equals(sort)?"导入":"最近");styleTabs();
    }
    void busy(boolean busy,String status){this.busy=busy;add.setEnabled(!busy);continueButton.setEnabled(!busy);cancel.setVisibility(busy?VISIBLE:GONE);message.setText(status);}
    boolean busy(){return busy;}
    void message(String value){message.setText(value);}
    void clearSearch(){search.clearFocus();((android.view.inputmethod.InputMethodManager)getContext().getSystemService(Activity.INPUT_METHOD_SERVICE)).hideSoftInputFromWindow(search.getWindowToken(),0);}
    void theme(NativeReader.Appearance p){colors=p;setBackgroundColor(p.canvas);count.setTextColor(p.muted);message.setTextColor(p.muted);search.setTextColor(p.ink);search.setHintTextColor(p.muted);search.setBackground(shape(p.panel));canvas.theme(p);themeButton.setText(p.dark()?"亮色":"暗色");styleTabs();}
    private GradientDrawable shape(int color){GradientDrawable d=new GradientDrawable();d.setColor(color);d.setCornerRadius(dp(9));return d;}
    private void styleTabs(){for(int i=0;i<modes.length;i++){modes[i].setBackgroundTintList(null);modes[i].setBackground(shape(i==mode?colors.accent:colors.button));modes[i].setTextColor(i==mode?colors.onAccent:colors.ink);modes[i].setContentDescription(new String[]{"列表模式","封面网格模式"}[i]+(i==mode?"，已选择":""));}}
}
