package xin.soymilk.readall

/** Real Kotlin gesture state machine; unchanged event sequences from the Java regression. */
object TouchSmoke {
    @JvmStatic fun main(args: Array<String>) {
        val events = ArrayList<Int>(); val t = TouchRouter(8f) { kind, _, _ -> events.add(kind) }
        t.down(10, 20); check(events == listOf(0)); t.up(10, 20, 0); check(events == listOf(0, 1))
        events.clear(); t.down(200, 200); t.move(80, 200); t.move(200, 200); t.up(200, 200, 0); check(1 !in events && 4 in events)
        events.clear(); t.down(20, 20); t.longPress(); t.move(100, 100); t.up(100, 100, 1000); check(events == listOf(0, 5, 6, 6, 7))
        events.clear(); t.down(10, 10); t.cancel(); t.up(10, 10, 0); check(events == listOf(0, 8))
        events.clear(); t.down(10, 10); t.up(10, 200, 500); check(events == listOf(0, 2, 3, 3, 4, 9))
        events.clear(); t.down(0, 0); t.move(0, 40); t.longPress(); t.up(0, 50, 0); check(5 !in events)
        println("PASS 6 Kotlin touch routing cases: taps, excursion, long selection, cancellation, final displacement and arbitration")
    }
}
