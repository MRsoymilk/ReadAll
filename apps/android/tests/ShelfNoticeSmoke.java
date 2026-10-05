package xin.soymilk.readall;

/** Deterministic lifecycle/timeout tests; uses the actual policy, no framework mocks. */
public final class ShelfNoticeSmoke {
    static void check(boolean value,String why){if(!value)throw new AssertionError(why);}
    public static void main(String[] args){
        ShelfNotice n=new ShelfNotice();n.update("导入失败",false,0,6000);check(n.visible(),"failure missing");n.expire(5999);check(n.visible(),"failure disappeared early");n.expire(6000);check(!n.visible()&&n.text().isEmpty(),"failure stuck at bottom");
        n.update("完成",false,7000,6000);n.update("新的失败",false,12000,6000);n.expire(13000);check(n.text().equals("新的失败"),"old timeout removed newer message");n.expire(18000);check(!n.visible(),"replacement timeout missing");
        n.update("正在导入",true,20000,6000);n.expire(900000);n.dismiss();check(n.visible()&&n.working()&&n.remaining(900000)==-1,"active import/cancel must stay visible");
        n.update("导入已取消",false,900000,6000);n.dismiss();check(!n.visible(),"close did not hide completed result");
        n.update("失败",false,1000000,6000);n.dismiss();n.expire(2000000);check(!n.visible(),"background/detach restored stale result");
        n.update("需要较长阅读时间",false,2000000,30000);n.expire(2029999);check(n.visible(),"accessibility timeout shortened");n.expire(2030000);check(!n.visible(),"extended timeout never ended");
        n.update("",false,3000000,6000);check(!n.visible(),"empty notice reserves footer");
        n.update("失败",false,Long.MAX_VALUE-100000,6000);check(n.remaining(Long.MAX_VALUE-100000)==6000,"timeout arithmetic wrong");
        System.out.println("PASS shelf notices: automatic expiry, replacement, dismiss, active-import preservation, lifecycle reset and accessibility timeout");
    }
}
