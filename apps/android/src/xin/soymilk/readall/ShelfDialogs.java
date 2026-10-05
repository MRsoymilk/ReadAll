package xin.soymilk.readall;

import android.app.Activity;
import android.app.Dialog;
import android.content.Context;
import android.content.res.ColorStateList;
import android.graphics.Color;
import android.graphics.drawable.ColorDrawable;
import android.text.InputFilter;
import android.view.ContextThemeWrapper;
import android.view.Gravity;
import android.view.View;
import android.view.Window;
import android.view.WindowManager;
import android.widget.*;
import java.util.function.Consumer;
import java.util.function.IntConsumer;

/** Small native bottom panels with a single primary action; no changes to shelf storage semantics. */
final class ShelfDialogs {
    private final Activity activity;
    private final Context context;
    private final NativeReader.Appearance colors;
    private final Dialog dialog;
    private final LinearLayout panel;
    private ShelfDialogs(Activity activity,NativeReader.Appearance colors,String title,String subtitle){
        this.activity=activity;this.colors=colors;context=new ContextThemeWrapper(activity,colors.dark()?android.R.style.Theme_Material_Dialog_NoActionBar:android.R.style.Theme_Material_Light_Dialog_NoActionBar);dialog=new Dialog(context);dialog.requestWindowFeature(Window.FEATURE_NO_TITLE);
        panel=new LinearLayout(context);panel.setOrientation(LinearLayout.VERTICAL);panel.setPadding(dp(20),dp(16),dp(20),dp(20));panel.setBackground(ShelfStyle.shape(context,colors.panel,24));
        View handle=new View(context);handle.setBackground(ShelfStyle.shape(context,colors.border,2));LinearLayout.LayoutParams hp=new LinearLayout.LayoutParams(dp(32),dp(4));hp.gravity=Gravity.CENTER_HORIZONTAL;hp.bottomMargin=dp(20);panel.addView(handle,hp);
        TextView heading=ShelfStyle.text(context,title,21,colors.ink,true);heading.setMaxLines(3);heading.setEllipsize(android.text.TextUtils.TruncateAt.END);panel.addView(heading);
        if(!subtitle.isEmpty()){TextView note=ShelfStyle.text(context,subtitle,14,colors.muted,false);note.setLineSpacing(dp(3),1);LinearLayout.LayoutParams lp=new LinearLayout.LayoutParams(-1,-2);lp.topMargin=dp(10);lp.bottomMargin=dp(14);panel.addView(note,lp);}
        ScrollView scroll=new ScrollView(context){@Override protected void onMeasure(int w,int h){int limit=(int)(getResources().getDisplayMetrics().heightPixels*.82f);super.onMeasure(w,MeasureSpec.makeMeasureSpec(Math.min(limit,MeasureSpec.getSize(h)>0?MeasureSpec.getSize(h):limit),MeasureSpec.AT_MOST));}};
        scroll.setFillViewport(false);scroll.setClipToPadding(false);scroll.addView(panel);dialog.setContentView(scroll);dialog.setCanceledOnTouchOutside(true);
    }
    private int dp(float value){return ShelfStyle.dp(context,value);}
    private void action(String label,int icon,Runnable run){Button row=ShelfStyle.button(context,label,()->{dialog.dismiss();run.run();});ShelfStyle.buttonTheme(row,colors,false);ShelfStyle.icon(row,icon,colors.muted,false);row.setCompoundDrawablePadding(dp(14));row.setGravity(Gravity.START|Gravity.CENTER_VERTICAL);row.setPadding(dp(8),0,dp(8),0);panel.addView(row,new LinearLayout.LayoutParams(-1,dp(52)));}
    private void buttons(String primary,Runnable run){LinearLayout row=new LinearLayout(context);row.setOrientation(LinearLayout.HORIZONTAL);Button cancel=ShelfStyle.button(context,"取消",dialog::dismiss),confirm=ShelfStyle.button(context,primary,()->{dialog.dismiss();run.run();});ShelfStyle.buttonTheme(cancel,colors,false);ShelfStyle.buttonTheme(confirm,colors,true);row.addView(cancel,new LinearLayout.LayoutParams(0,dp(48),1));LinearLayout.LayoutParams lp=new LinearLayout.LayoutParams(0,dp(48),1);lp.leftMargin=dp(12);row.addView(confirm,lp);LinearLayout.LayoutParams outer=new LinearLayout.LayoutParams(-1,-2);outer.topMargin=dp(18);panel.addView(row,outer);}
    private Dialog show(){dialog.show();Window window=dialog.getWindow();if(window!=null){window.setBackgroundDrawable(new ColorDrawable(Color.TRANSPARENT));window.setDimAmount(.28f);window.addFlags(WindowManager.LayoutParams.FLAG_DIM_BEHIND);window.setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE);window.setGravity(Gravity.BOTTOM|Gravity.CENTER_HORIZONTAL);WindowManager.LayoutParams lp=window.getAttributes();lp.width=Math.min(activity.getResources().getDisplayMetrics().widthPixels-dp(24),dp(560));lp.height=-2;lp.y=dp(12);window.setAttributes(lp);}return dialog;}
    static Dialog actions(Activity activity,NativeReader.Appearance p,ShelfStore.Book book,IntConsumer action){
        ShelfDialogs sheet=new ShelfDialogs(activity,p,book.label(),(book.author.isEmpty()?book.format:book.author+" · "+book.format)+" · "+book.progress());
        String[] labels={"打开阅读",book.pinned?"取消置顶":"置顶图书","修改显示书名","重新读取封面","移出书库"};int[] icons={ShelfIcon.BOOK,ShelfIcon.PIN,ShelfIcon.EDIT,ShelfIcon.REFRESH,ShelfIcon.TRASH};
        for(int i=0;i<labels.length;i++){final int which=i;if(i==4){View line=new View(sheet.context);line.setBackgroundColor(p.border);LinearLayout.LayoutParams sep=new LinearLayout.LayoutParams(-1,1);sep.topMargin=sheet.dp(8);sep.bottomMargin=sheet.dp(8);sheet.panel.addView(line,sep);}sheet.action(labels[i],icons[i],()->action.accept(which));}return sheet.show();
    }
    static Dialog rename(Activity activity,NativeReader.Appearance p,ShelfStore.Book book,Consumer<String> save){
        ShelfDialogs sheet=new ShelfDialogs(activity,p,"修改显示书名","只修改书库中的名称，不改动原书。留空恢复原书名。");EditText input=new EditText(sheet.context);input.setText(book.label());input.setSingleLine(true);input.setTextSize(16);input.setTextColor(p.ink);input.setHintTextColor(p.muted);input.setHint("书名");input.setSelectAllOnFocus(true);input.setFilters(new InputFilter[]{new InputFilter.LengthFilter(512)});input.setPadding(sheet.dp(14),sheet.dp(10),sheet.dp(14),sheet.dp(10));input.setBackground(ShelfStyle.touch(sheet.context,p.canvas,p.accent,12));input.setMinimumHeight(sheet.dp(52));input.setContentDescription("显示书名");sheet.panel.addView(input,new LinearLayout.LayoutParams(-1,-2));sheet.buttons("保存",()->save.accept(input.getText().toString()));return sheet.show();
    }
    static Dialog remove(Activity activity,NativeReader.Appearance p,ShelfStore.Book book,Consumer<Boolean> remove){
        ShelfDialogs sheet=new ShelfDialogs(activity,p,"移出书库？",book.label()+"\n\n不会删除系统中的原书，也不会清除阅读进度、书签、高亮或笔记。");CheckBox clear=new CheckBox(sheet.context);clear.setText("同时清理应用内副本和封面");clear.setTextSize(14);clear.setTextColor(p.ink);clear.setButtonTintList(new ColorStateList(new int[][]{new int[]{android.R.attr.state_checked},new int[]{}},new int[]{p.accent,p.muted}));clear.setPadding(0,sheet.dp(8),0,sheet.dp(8));clear.setMinimumHeight(sheet.dp(48));clear.setChecked(false);sheet.panel.addView(clear);sheet.buttons("移出书库",()->remove.accept(clear.isChecked()));return sheet.show();
    }
}
