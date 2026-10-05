package xin.soymilk.readall

/** Shared draw/hit geometry; IDs and floating-point rounding are preserved across migration. */
object ShelfGeometry {
    const val LIST = 0; const val COVERS = 1
    const val EDGE = 20f; const val GAP = 18f; const val TOUCH = 48f; const val COVER_RATIO = 1.40f
    @JvmStatic fun mode(value: Int) = if (value == LIST) LIST else COVERS
    @JvmStatic fun columns(width: Float) = ((maxOf(1f, width) - 2 * EDGE + GAP) / (128 + GAP)).toInt().coerceIn(1, 6)
    @JvmStatic fun cellWidth(width: Float): Float { val cols = columns(width); return maxOf(32f, (width - 2 * EDGE - (cols - 1) * GAP) / cols) }
    @JvmStatic fun pitch(width: Float, mode: Int) = if (mode == LIST) 112f else cellWidth(width) * COVER_RATIO + 88
    class Tile(width: Float, mode: Int, index: Int, offset: Float) {
        @JvmField val left: Float; @JvmField val top: Float; @JvmField val right: Float; @JvmField val bottom: Float
        @JvmField val menuLeft: Float; @JvmField val menuTop: Float; @JvmField val menuRight: Float; @JvmField val menuBottom: Float
        init {
            val cols = if (mode == LIST) 1 else columns(width)
            left = if (mode == LIST) EDGE else EDGE + (index % cols) * (cellWidth(width) + GAP)
            top = 12 + (index / cols) * pitch(width, mode) - offset
            right = if (mode == LIST) width - EDGE else left + cellWidth(width)
            bottom = top + pitch(width, mode) - 12; menuLeft = right - TOUCH; menuTop = top; menuRight = right; menuBottom = top + TOUCH
        }
    }
    @JvmStatic fun maxOffset(width: Float, height: Float, count: Int, mode: Int): Float { val rows = if (mode == LIST) count else (count + columns(width) - 1) / columns(width); return maxOf(0f, rows * pitch(width, mode) + 24 - height) }
    @JvmStatic fun clamp(value: Float, max: Float) = if (value.isFinite()) maxOf(0f, minOf(max, value)) else 0f
    private fun rowPosition(offset: Float, pitch: Float): Double { val units = offset.toDouble() / pitch; val nearest = Math.rint(units); return if (Math.abs(units - nearest) < 0.0001) nearest else units }
    @JvmStatic fun first(width: Float, count: Int, mode: Int, offset: Float) = if (count == 0) 0 else minOf(count, rowPosition(offset, pitch(width, mode)).toInt() * if (mode == LIST) 1 else columns(width))
    @JvmStatic fun end(width: Float, height: Float, count: Int, mode: Int, offset: Float): Int { val rows = Math.ceil((height / pitch(width, mode)).toDouble()).toInt() + 2; return minOf(count, first(width, count, mode, offset) + rows * if (mode == LIST) 1 else columns(width)) }
    @JvmStatic fun restoreOffset(width: Float, mode: Int, index: Int, previousWidth: Float, previousMode: Int, previousOffset: Float): Float {
        if (index < 0) return 0f
        val units = rowPosition(previousOffset, pitch(previousWidth, previousMode))
        val fraction = if (mode == previousMode) (units - Math.floor(units)).toFloat() else 0f
        val row = if (mode == LIST) index else index / columns(width)
        return (row + fraction) * pitch(width, mode)
    }
}
