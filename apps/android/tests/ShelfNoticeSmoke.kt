package xin.soymilk.readall

object ShelfNoticeSmoke {
    @JvmStatic fun main(args: Array<String>) {
        val n = ShelfNotice(); n.update("导入失败", false, 0, 6000); check(n.visible()); n.expire(5999); check(n.visible()); n.expire(6000); check(!n.visible() && n.text().isEmpty())
        n.update("完成", false, 7000, 6000); n.update("新的失败", false, 12000, 6000); n.expire(13000); check(n.text() == "新的失败"); n.expire(18000); check(!n.visible())
        n.update("正在导入", true, 20000, 6000); n.expire(900000); n.dismiss(); check(n.visible() && n.working() && n.remaining(900000) == -1L)
        n.update("导入已取消", false, 900000, 6000); n.dismiss(); check(!n.visible())
        n.update("失败", false, 1000000, 6000); n.dismiss(); n.expire(2000000); check(!n.visible())
        n.update("需要较长阅读时间", false, 2000000, 30000); n.expire(2029999); check(n.visible()); n.expire(2030000); check(!n.visible())
        n.update("", false, 3000000, 6000); check(!n.visible()); n.update("失败", false, Long.MAX_VALUE - 100000, 6000); check(n.remaining(Long.MAX_VALUE - 100000) == 6000L)
        println("PASS Kotlin shelf notices: expiry, replacement, dismiss, active-work retention, lifecycle and accessibility timeout")
    }
}
