package xin.soymilk.readall

/** Platform-free gesture state machine; coordinates are shared-renderer logical units. */
class TouchRouter(private val slop: Float, private val sink: Sink) {
    fun interface Sink { fun send(kind: Int, x: Int, y: Int) }
    private var mode = 0 // 0 idle, 1 undecided, 2 page/list drag, 3 long-press selection.
    private var startX = 0; private var startY = 0; private var lastX = 0; private var lastY = 0
    fun active() = mode != 0
    fun selecting() = mode == 3
    fun down(x: Int, y: Int) { if (mode != 0) cancel(); mode = 1; startX = x; lastX = x; startY = y; lastY = y; sink.send(0, x, y) }
    fun move(x: Int, y: Int) {
        if (mode == 0) return
        lastX = x; lastY = y
        if (mode == 1 && (Math.abs(x.toLong() - startX) >= slop || Math.abs(y.toLong() - startY) >= slop)) { mode = 2; sink.send(2, startX, startY) }
        if (mode == 2) sink.send(3, x, y) else if (mode == 3) sink.send(6, x, y)
    }
    fun longPress() { if (mode == 1) { mode = 3; sink.send(5, startX, startY) } }
    fun up(x: Int, y: Int, flingY: Int) {
        if (mode == 0) return
        if (mode == 1) move(x, y) // Include a final displacement even when Android omitted MOVE.
        when (mode) {
            1 -> sink.send(1, x, y)
            2 -> { sink.send(3, x, y); sink.send(4, x, y); if (flingY != 0) sink.send(9, 0, flingY) }
            3 -> { sink.send(6, x, y); sink.send(7, x, y) }
        }
        mode = 0
    }
    fun cancel() { if (mode != 0) sink.send(8, lastX, lastY); mode = 0 }
}
