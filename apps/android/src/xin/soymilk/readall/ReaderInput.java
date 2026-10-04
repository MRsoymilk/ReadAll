package xin.soymilk.readall;

import android.text.Editable;
import android.text.Selection;
import android.text.SpannableStringBuilder;
import android.view.KeyEvent;
import android.view.View;
import android.view.inputmethod.BaseInputConnection;
import java.nio.charset.StandardCharsets;

/** Real Android composing buffer over the shared Rust search/note input field. */
final class ReaderInput extends BaseInputConnection {
    interface Changed { void set(String mode,String text); }
    private final Editable text;
    private final String mode;
    private final Changed changed;
    private final Runnable submit;
    private String pending;
    ReaderInput(View view,String mode,String initial,Changed changed,Runnable submit){
        super(view,true);this.mode=mode;this.changed=changed;this.submit=submit;text=new SpannableStringBuilder(initial);Selection.setSelection(text,text.length());
    }
    @Override public Editable getEditable(){return text;}
    private void update(){
        int limit="search".equals(mode)?1024:8192;
        while(text.toString().getBytes(StandardCharsets.UTF_8).length>limit && text.length()>0){int end=text.length(),start=Character.offsetByCodePoints(text,end,-1);text.delete(start,end);}
        pending=text.toString();changed.set(mode,pending);
    }
    void nativeText(String value){
        if(pending!=null){if(value.equals(pending))pending=null;return;}
        if(!text.toString().equals(value)){text.replace(0,text.length(),value);Selection.setSelection(text,text.length());}
    }
    @Override public boolean commitText(CharSequence value,int cursor){boolean ok=super.commitText(value,cursor);update();return ok;}
    @Override public boolean setComposingText(CharSequence value,int cursor){boolean ok=super.setComposingText(value,cursor);update();return ok;}
    @Override public boolean finishComposingText(){boolean ok=super.finishComposingText();update();return ok;}
    @Override public boolean deleteSurroundingText(int before,int after){boolean ok=super.deleteSurroundingText(before,after);update();return ok;}
    @Override public boolean deleteSurroundingTextInCodePoints(int before,int after){boolean ok=super.deleteSurroundingTextInCodePoints(before,after);update();return ok;}
    @Override public boolean performEditorAction(int action){finishComposingText();submit.run();return true;}
    @Override public boolean sendKeyEvent(KeyEvent event){
        if(event.getAction()!=KeyEvent.ACTION_DOWN)return true;
        if(event.getKeyCode()==KeyEvent.KEYCODE_DEL)return deleteSurroundingTextInCodePoints(1,0);
        if(event.getKeyCode()==KeyEvent.KEYCODE_ENTER){submit.run();return true;}
        int cp=event.getUnicodeChar();if(cp!=0)return commitText(new String(Character.toChars(cp)),1);
        return super.sendKeyEvent(event);
    }
}
