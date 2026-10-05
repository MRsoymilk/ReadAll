package xin.soymilk.readall

import android.graphics.Canvas
import android.graphics.ColorFilter
import android.graphics.Paint
import android.graphics.Path
import android.graphics.PixelFormat
import android.graphics.drawable.Drawable

/** Original line paths are retained exactly; constructed once, with no font/network dependency. */
class ShelfIcon(kind: Int, private val color: Int, private val size: Int) : Drawable() {
    private val path = Path()
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply { style = Paint.Style.STROKE; strokeWidth = 1.7f; strokeCap = Paint.Cap.ROUND; strokeJoin = Paint.Join.ROUND }
    private var opacity = 255
    init {
        when (kind) {
            ADD -> { line(12f, 5f, 12f, 19f); line(5f, 12f, 19f, 12f) }
            SEARCH -> { path.addCircle(10.5f, 10.5f, 6.5f, Path.Direction.CW); line(15.5f, 15.5f, 21f, 21f) }
            CLOSE -> { line(6f, 6f, 18f, 18f); line(18f, 6f, 6f, 18f) }
            LIST -> for (y in 6..18 step 6) { line(4f, y.toFloat(), 5f, y.toFloat()); line(9f, y.toFloat(), 20f, y.toFloat()) }
            GRID -> for (x in 4..14 step 10) for (y in 4..14 step 10) path.addRoundRect(x.toFloat(), y.toFloat(), x + 6f, y + 6f, 1f, 1f, Path.Direction.CW)
            SORT -> { line(5f, 6f, 19f, 6f); line(5f, 12f, 15f, 12f); line(5f, 18f, 11f, 18f) }
            SUN -> { path.addCircle(12f, 12f, 4f, Path.Direction.CW); repeat(8) { n -> val a = n * Math.PI / 4; line(12 + 7 * Math.cos(a).toFloat(), 12 + 7 * Math.sin(a).toFloat(), 12 + 9 * Math.cos(a).toFloat(), 12 + 9 * Math.sin(a).toFloat()) } }
            MOON -> { path.moveTo(19.8f, 15f); path.cubicTo(15f, 17f, 7f, 10f, 10f, 3.5f); path.cubicTo(-1f, 6f, 3f, 24f, 15f, 20f); path.quadTo(18f, 19f, 19.8f, 15f) }
            BOOK -> { path.moveTo(12f, 6f); path.cubicTo(9f, 3f, 5f, 3f, 3f, 4f); path.lineTo(3f, 19f); path.cubicTo(6f, 18f, 9f, 18f, 12f, 21f); path.cubicTo(15f, 18f, 18f, 18f, 21f, 19f); path.lineTo(21f, 4f); path.cubicTo(18f, 3f, 15f, 3f, 12f, 6f); line(12f, 6f, 12f, 21f) }
            ARROW -> { line(5f, 12f, 19f, 12f); line(14f, 7f, 19f, 12f); line(19f, 12f, 14f, 17f) }
            PIN -> { path.moveTo(8f, 3f); path.lineTo(16f, 3f); path.lineTo(15f, 10f); path.lineTo(19f, 15f); path.lineTo(5f, 15f); path.lineTo(9f, 10f); path.close(); line(12f, 15f, 12f, 22f) }
            EDIT -> { path.moveTo(4f, 16f); path.lineTo(16f, 4f); path.lineTo(20f, 8f); path.lineTo(8f, 20f); path.lineTo(3f, 21f); path.close(); line(13f, 7f, 17f, 11f) }
            REFRESH -> { path.addArc(4f, 4f, 20f, 20f, 40f, 290f); line(20f, 4f, 20f, 10f); line(14f, 10f, 20f, 10f) }
            TRASH -> { line(4f, 6f, 20f, 6f); path.moveTo(8f, 6f); path.lineTo(8f, 3f); path.lineTo(16f, 3f); path.lineTo(16f, 6f); path.moveTo(6f, 6f); path.lineTo(7f, 21f); path.lineTo(17f, 21f); path.lineTo(18f, 6f); line(10f, 10f, 10f, 17f); line(14f, 10f, 14f, 17f) }
            MORE -> for (x in 5..19 step 7) path.addCircle(x.toFloat(), 12f, .75f, Path.Direction.CW)
            else -> throw IllegalArgumentException("Unknown shelf icon")
        }
        setBounds(0, 0, size, size)
    }
    private fun line(x: Float, y: Float, endX: Float, endY: Float) { path.moveTo(x, y); path.lineTo(endX, endY) }
    override fun draw(canvas: Canvas) { val save = canvas.save(); canvas.translate(bounds.left.toFloat(), bounds.top.toFloat()); canvas.scale(bounds.width() / 24f, bounds.height() / 24f); paint.color = color; paint.alpha = opacity; canvas.drawPath(path, paint); canvas.restoreToCount(save) }
    override fun setAlpha(value: Int) { opacity = value.coerceIn(0, 255); invalidateSelf() }
    override fun setColorFilter(filter: ColorFilter?) { paint.colorFilter = filter; invalidateSelf() }
    @Suppress("OVERRIDE_DEPRECATION") override fun getOpacity() = PixelFormat.TRANSLUCENT
    override fun getIntrinsicWidth() = size
    override fun getIntrinsicHeight() = size
    companion object { const val ADD = 0; const val SEARCH = 1; const val CLOSE = 2; const val LIST = 3; const val GRID = 4; const val SORT = 5; const val SUN = 6; const val MOON = 7; const val BOOK = 8; const val ARROW = 9; const val PIN = 10; const val EDIT = 11; const val REFRESH = 12; const val TRASH = 13; const val MORE = 14 }
}
