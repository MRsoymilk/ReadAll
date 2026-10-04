package xin.soymilk.readall;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
/** Real Java state-machine tests, independent of Android event simulation. */
public final class TouchSmoke {
    private static void check(boolean ok,String why){if(!ok)throw new AssertionError(why);}
    public static void main(String[] args){
        List<Integer> events=new ArrayList<>();TouchRouter t=new TouchRouter(8,(kind,x,y)->events.add(kind));
        t.down(10,20);check(events.equals(Arrays.asList(0)),"press activated UI before release");t.up(10,20,0);check(events.equals(Arrays.asList(0,1)),"tap mapping");
        events.clear();t.down(200,200);t.move(80,200);t.move(200,200);t.up(200,200,0);check(!events.contains(1)&&events.contains(4),"excursion became a tap");
        events.clear();t.down(20,20);t.longPress();t.move(100,100);t.up(100,100,1000);check(events.equals(Arrays.asList(0,5,6,6,7)),"long selection also paged/flung");
        events.clear();t.down(10,10);t.cancel();t.up(10,10,0);check(events.equals(Arrays.asList(0,8)),"cancel generated click");
        events.clear();t.down(10,10);t.up(10,200,500);check(events.equals(Arrays.asList(0,2,3,3,4,9)),"release-only displacement lost drag");
        events.clear();t.down(0,0);t.move(0,40);t.longPress();t.up(0,50,0);check(!events.contains(5),"late long press stole a page/list drag");
        System.out.println("PASS 6 touch routing cases: delayed taps, drag excursion, long selection, cancellation, final displacement and long-press arbitration");
    }
}
