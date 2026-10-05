package xin.soymilk.readall;

/** Actual presentation policy with a deterministic clock; no Android framework stubs. */
public final class LoadingFeedbackSmoke {
    private static void expect(int actual,int expected,String why){if(actual!=expected)throw new AssertionError(why+": "+actual+" != "+expected);}
    public static void main(String[] args){
        LoadingFeedback p=new LoadingFeedback();
        expect(p.update(false,true,0),LoadingFeedback.INITIAL,"initial open has progress and cancel");
        expect(p.update(false,false,100),LoadingFeedback.INITIAL,"native completion is not displayed pixels");
        expect(p.update(true,false,110),LoadingFeedback.NONE,"first displayed page hides the panel");
        expect(p.update(true,true,120),LoadingFeedback.NONE,"do not flash on normal frame work");
        expect(p.update(true,true,319),LoadingFeedback.NONE,"wait 200 ms before corner spinner");
        expect(p.update(true,true,320),LoadingFeedback.CORNER,"slow reflow uses only the corner");
        expect(p.update(true,true,5000),LoadingFeedback.CORNER,"even very slow work never restores a modal");
        expect(p.update(true,false,5001),LoadingFeedback.NONE,"finished long load stops immediately");

        p.reset();
        expect(p.update(true,true,0),LoadingFeedback.NONE,"zero is a valid monotonic start");
        expect(p.update(true,true,200),LoadingFeedback.CORNER,"show after delay");
        expect(p.update(true,false,201),LoadingFeedback.CORNER,"avoid a one-frame blink");
        expect(p.update(true,false,359),LoadingFeedback.CORNER,"minimum visible interval");
        expect(p.update(true,false,360),LoadingFeedback.NONE,"no stale spinner after minimum interval");

        p.reset();
        for(int i=0;i<20;i++){
            expect(p.update(true,true,1000+i*100),LoadingFeedback.NONE,"short bursts start without spinner");
            expect(p.update(true,false,1090+i*100),LoadingFeedback.NONE,"separate frame bursts do not accumulate");
        }
        p.reset();
        p.update(true,true,0);p.update(true,true,200);p.update(true,false,250);
        expect(p.update(true,true,300),LoadingFeedback.CORNER,"a burst during the visible interval must not blink");
        expect(p.update(true,false,400),LoadingFeedback.NONE,"reset after burst ends");

        // Pause, error, cancellation and new-book paths all reset the same policy.
        for(int i=0;i<4;i++){
            p.reset();p.update(true,true,0);p.update(true,true,200);p.reset();
            expect(p.update(true,false,201),LoadingFeedback.NONE,"lifecycle reset ignores old minimum visibility");
            expect(p.update(true,true,202),LoadingFeedback.NONE,"new work gets its own timer");
        }
        p.reset();
        expect(p.update(false,true,0),LoadingFeedback.INITIAL,"new book initial load");
        expect(p.update(false,true,10000),LoadingFeedback.INITIAL,"long first load still cancellable");
        expect(p.update(true,true,10001),LoadingFeedback.NONE,"first display starts a new non-modal timer");
        expect(p.update(true,true,10201),LoadingFeedback.CORNER,"subsequent preparation is small");
        expect(p.update(false,true,10202),LoadingFeedback.INITIAL,"switching books restores initial feedback");
        expect(p.update(true,false,10203),LoadingFeedback.NONE,"previous book spinner is not carried over");

        // Every possible busy transition after display is restricted to NONE/CORNER.
        for(int i=0;i<10000;i++){
            int mode=p.update(true,(i%29)<19,20000L+i*17);
            if(mode==LoadingFeedback.INITIAL)throw new AssertionError("reading restored the central panel");
        }
        System.out.println("PASS loading feedback: first displayed frame, short bursts, delayed corner spinner, stable visibility, completion, long work, lifecycle resets and no modal during reading");
    }
}
