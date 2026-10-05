package xin.soymilk.readall;

import android.app.Activity;
import android.content.res.ColorStateList;
import android.graphics.drawable.ColorDrawable;
import android.os.Build;
import android.view.View;
import android.view.ViewGroup;
import android.view.Window;
import android.view.WindowInsetsController;
import android.widget.Button;
import android.widget.ProgressBar;
import android.widget.TextView;

/** Platform surfaces use the Rust palette; no Android-only copy of RGB values. */
final class AndroidTheme {
    private AndroidTheme(){}
    static void apply(Activity activity,ViewGroup root,View home,View loading,ReaderView page,ProgressBar progress,PageLoadingIndicator pageLoading,NativeReader.Appearance p){
        root.setBackgroundColor(p.canvas);home.setBackgroundColor(p.canvas);loading.setBackgroundColor(p.panel);page.background(p.page);
        if(Build.VERSION.SDK_INT>=29)root.setForceDarkAllowed(false);
        tint(root,p);
        progress.setProgressTintList(ColorStateList.valueOf(p.accent));
        progress.setProgressBackgroundTintList(ColorStateList.valueOf(p.border));
        progress.setIndeterminateTintList(ColorStateList.valueOf(p.accent));
        pageLoading.setIndeterminateTintList(ColorStateList.valueOf(p.accent));
        Window window=activity.getWindow();window.setBackgroundDrawable(new ColorDrawable(p.canvas));
        // API 35 edge-to-edge uses root insets as the bar backdrop. Older versions
        // still honor these colors. Icons must be set separately on every switch.
        window.setStatusBarColor(p.canvas);window.setNavigationBarColor(p.canvas);
        if(Build.VERSION.SDK_INT>=29){window.setStatusBarContrastEnforced(false);window.setNavigationBarContrastEnforced(false);}
        if(Build.VERSION.SDK_INT>=30){
            WindowInsetsController controller=window.getInsetsController();
            int mask=WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS|WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS;
            if(controller!=null)controller.setSystemBarsAppearance(p.dark()?0:mask,mask);
        }else{
            View decor=window.getDecorView();int mask=View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR|View.SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR;
            decor.setSystemUiVisibility((decor.getSystemUiVisibility()&~mask)|(p.dark()?0:mask));
        }
    }
    private static void tint(View view,NativeReader.Appearance p){
        if(view instanceof Button){
            Button b=(Button)view;b.setTextColor(p.ink);
            b.setBackgroundTintList(new ColorStateList(new int[][]{new int[]{android.R.attr.state_pressed},new int[]{}},new int[]{p.hover,p.button}));
        }else if(view instanceof TextView)((TextView)view).setTextColor(p.ink);
        if(view instanceof ViewGroup){ViewGroup group=(ViewGroup)view;for(int i=0;i<group.getChildCount();i++)tint(group.getChildAt(i),p);}
    }
}
