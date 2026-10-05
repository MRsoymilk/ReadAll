package xin.soymilk.readall;

import android.content.Context;
import android.content.res.ColorStateList;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.graphics.drawable.RippleDrawable;
import android.graphics.drawable.StateListDrawable;
import android.view.Gravity;
import android.view.View;
import android.widget.Button;
import android.widget.TextView;

/** Android-only component shapes. All colors come from the existing shared reader palette. */
final class ShelfStyle {
    static final Typeface REGULAR=Typeface.create("sans-serif",Typeface.NORMAL),MEDIUM=Typeface.create("sans-serif-medium",Typeface.NORMAL);
    static int dp(Context c,float value){return Math.round(value*c.getResources().getDisplayMetrics().density);}
    static GradientDrawable shape(Context c,int fill,float radius){GradientDrawable d=new GradientDrawable();d.setColor(fill);d.setCornerRadius(dp(c,radius));return d;}
    static RippleDrawable touch(Context c,int fill,int ink,float radius){
        GradientDrawable normal=shape(c,fill,radius),focus=shape(c,fill,radius);focus.setStroke(dp(c,2),ink);
        StateListDrawable content=new StateListDrawable();content.addState(new int[]{android.R.attr.state_focused},focus);content.addState(new int[]{},normal);
        return new RippleDrawable(ColorStateList.valueOf((ink&0x00ffffff)|0x24000000),content,shape(c,0xffffffff,radius));
    }
    static Button button(Context c,String label,Runnable action){
        Button b=new Button(c);b.setText(label);b.setTextSize(14);b.setTypeface(MEDIUM);b.setAllCaps(false);b.setSingleLine(true);b.setEllipsize(android.text.TextUtils.TruncateAt.END);
        b.setMinWidth(0);b.setMinimumWidth(0);b.setMinHeight(dp(c,48));b.setMinimumHeight(dp(c,48));b.setIncludeFontPadding(false);b.setPadding(dp(c,12),0,dp(c,12),0);b.setGravity(Gravity.CENTER);
        b.setStateListAnimator(null);b.setElevation(0);b.setOnClickListener(v->action.run());return b;
    }
    static void buttonTheme(Button b,NativeReader.Appearance p,boolean primary){
        int ink=primary?p.onAccent:p.ink;b.setTextColor(new ColorStateList(new int[][]{new int[]{-android.R.attr.state_enabled},new int[]{}},new int[]{p.muted,ink}));
        b.setBackgroundTintList(null);b.setBackground(touch(b.getContext(),primary?p.accent:p.panel,primary?p.onAccent:p.accent,14));
    }
    static void icon(Button b,int kind,int color,boolean iconOnly){
        int size=dp(b.getContext(),20);b.setCompoundDrawablePadding(iconOnly?0:dp(b.getContext(),6));b.setCompoundDrawablesRelative(new ShelfIcon(kind,color,size),null,null,null);
        if(iconOnly){b.setText("");b.setPadding(dp(b.getContext(),14),0,dp(b.getContext(),14),0);}
    }
    static TextView text(Context c,String value,int size,int color,boolean medium){TextView t=new TextView(c);t.setText(value);t.setTextSize(size);t.setTextColor(color);t.setTypeface(medium?MEDIUM:REGULAR);t.setIncludeFontPadding(false);t.setGravity(Gravity.CENTER_VERTICAL);return t;}
    static void singleLine(TextView text){text.setSingleLine(true);text.setEllipsize(android.text.TextUtils.TruncateAt.END);}
    private ShelfStyle(){}
}
