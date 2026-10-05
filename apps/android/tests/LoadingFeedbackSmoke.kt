package xin.soymilk.readall

object LoadingFeedbackSmoke {
    @JvmStatic fun main(args: Array<String>) {
        val p = LoadingFeedback()
        fun expect(page: Boolean, busy: Boolean, now: Long, result: Int) { check(p.update(page, busy, now) == result) { "loading feedback at $now" } }
        expect(false, true, 0, LoadingFeedback.INITIAL); expect(false, false, 100, LoadingFeedback.INITIAL); expect(true, false, 110, LoadingFeedback.NONE)
        expect(true, true, 120, LoadingFeedback.NONE); expect(true, true, 319, LoadingFeedback.NONE); expect(true, true, 320, LoadingFeedback.CORNER)
        expect(true, true, 5000, LoadingFeedback.CORNER); expect(true, false, 5001, LoadingFeedback.NONE)
        p.reset(); expect(true, true, 0, LoadingFeedback.NONE); expect(true, true, 200, LoadingFeedback.CORNER); expect(true, false, 201, LoadingFeedback.CORNER)
        expect(true, false, 359, LoadingFeedback.CORNER); expect(true, false, 360, LoadingFeedback.NONE)
        p.reset(); repeat(20) { expect(true, true, 1000L + it * 100, LoadingFeedback.NONE); expect(true, false, 1090L + it * 100, LoadingFeedback.NONE) }
        p.reset(); p.update(true, true, 0); p.update(true, true, 200); p.update(true, false, 250)
        expect(true, true, 300, LoadingFeedback.CORNER); expect(true, false, 400, LoadingFeedback.NONE)
        repeat(4) { p.reset(); p.update(true, true, 0); p.update(true, true, 200); p.reset(); expect(true, false, 201, LoadingFeedback.NONE); expect(true, true, 202, LoadingFeedback.NONE) }
        p.reset(); expect(false, true, 0, LoadingFeedback.INITIAL); expect(false, true, 10000, LoadingFeedback.INITIAL)
        expect(true, true, 10001, LoadingFeedback.NONE); expect(true, true, 10201, LoadingFeedback.CORNER); expect(false, true, 10202, LoadingFeedback.INITIAL); expect(true, false, 10203, LoadingFeedback.NONE)
        repeat(10000) { check(p.update(true, it % 29 < 19, 20000L + it * 17) != LoadingFeedback.INITIAL) }
        println("PASS Kotlin loading feedback: first displayed frame, short bursts, delay, visibility, completion, cancellation and lifecycle reset")
    }
}
