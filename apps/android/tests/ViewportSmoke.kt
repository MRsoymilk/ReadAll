package xin.soymilk.readall

object ViewportSmoke {
    @JvmStatic fun main(args: Array<String>) {
        var v = ReaderViewport.sizes(1080, 2400, 2.75f); check(v.contentEquals(intArrayOf(393, 873, 1080, 2400)))
        var p = ReaderViewport.point(540f, 1200f, 1080, 2400, 1080, 2400, v[0], v[1]); check(kotlin.math.abs(p[0] - v[0] / 2f) <= .5f && kotlin.math.abs(p[1] - v[1] / 2f) <= .5f)
        for (screen in arrayOf(intArrayOf(1080, 2160), intArrayOf(2400, 1080), intArrayOf(7680, 4320), intArrayOf(320, 600))) {
            v = ReaderViewport.sizes(screen[0], screen[1], 2.75f); check(v[2].toLong() * v[3] <= ReaderViewport.MAX_PIXELS)
            p = ReaderViewport.point(screen[0] / 2f, screen[1] / 2f, screen[0], screen[1], v[2], v[3], v[0], v[1]); check(kotlin.math.abs(p[0] - v[0] / 2f) <= .51f && kotlin.math.abs(p[1] - v[1] / 2f) <= .51f)
        }
        JniSmoke.expect<IllegalArgumentException>("zero viewport accepted") { ReaderViewport.sizes(0, 200, 1f) }
        println("PASS Kotlin viewport: native pixels, bounded fallback, logical touch and orientation mapping")
    }
}
