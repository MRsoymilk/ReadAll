package xin.soymilk.readall

/** Layout units never double as bitmap pixels. Keep native resolution within a bounded budget. */
object ReaderViewport {
    const val MAX_PIXELS = 4L * 1024 * 1024
    @JvmStatic fun sizes(width: Int, height: Int, density: Float): IntArray {
        require(width in 1..16384 && height in 1..16384 && density.isFinite() && density > 0) { "Invalid viewport" }
        val lw = Math.round(width / maxOf(1f, density)).coerceIn(320, 1024)
        val lh = Math.round(height * lw.toFloat() / width).coerceIn(256, 2048)
        val scale = minOf(1.0, Math.sqrt(MAX_PIXELS / (width.toDouble() * height)))
        return intArrayOf(lw, lh, maxOf(1, Math.floor(width * scale).toInt()), maxOf(1, Math.floor(height * scale).toInt()))
    }
    @JvmStatic fun point(x: Float, y: Float, viewWidth: Int, viewHeight: Int, pixelWidth: Int, pixelHeight: Int, logicalWidth: Int, logicalHeight: Int): IntArray {
        val scale = minOf(viewWidth.toFloat() / pixelWidth, viewHeight.toFloat() / pixelHeight)
        val left = (viewWidth - pixelWidth * scale) / 2; val top = (viewHeight - pixelHeight * scale) / 2
        return intArrayOf(Math.round((x - left) / scale * logicalWidth / pixelWidth), Math.round((y - top) / scale * logicalHeight / pixelHeight))
    }
}
