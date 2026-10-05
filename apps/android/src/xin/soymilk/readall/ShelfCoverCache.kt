package xin.soymilk.readall

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.os.Handler
import android.os.Looper
import android.util.LruCache
import java.io.File
import java.util.concurrent.Executors

/** Bounded immutable thumbnails; all filesystem/decoding work stays off onDraw. */
class ShelfCoverCache(private val directory: File, private val changed: Runnable) {
    private val main = Handler(Looper.getMainLooper())
    private val worker = Executors.newSingleThreadExecutor()
    private val pending = HashSet<String>(); private val missing = HashSet<String>(); private val revisions = HashMap<String, Int>()
    private val cache = object : LruCache<String, Bitmap>(20 * 1024 * 1024) { override fun sizeOf(key: String, image: Bitmap) = image.allocationByteCount }
    private var closed = false
    fun get(id: String): Bitmap? {
        val image = cache.get(id)
        if (image != null || closed || id in missing || id in pending || pending.size >= 12) return image
        if (!id.matches(Regex("[0-9a-f]{64}"))) return null
        val revision = revisions[id] ?: 0; pending.add(id)
        worker.execute {
            var result: Bitmap? = null
            try {
                val file = File(directory, "$id.png")
                if (file.isFile && file.length() <= 2 * 1024 * 1024) {
                    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }; BitmapFactory.decodeFile(file.path, bounds)
                    if (bounds.outWidth in 1..384 && bounds.outHeight in 1..512) {
                        result = BitmapFactory.decodeFile(file.path, BitmapFactory.Options().apply { inPreferredConfig = Bitmap.Config.ARGB_8888 })
                        result?.density = Bitmap.DENSITY_NONE
                    }
                }
            } catch (_: RuntimeException) { } catch (_: OutOfMemoryError) { }
            val ready = result
            main.post {
                pending.remove(id)
                when {
                    closed -> ready?.recycle()
                    revision != (revisions[id] ?: 0) -> { ready?.recycle(); changed.run() }
                    else -> { if (ready == null) missing.add(id) else cache.put(id, ready); changed.run() }
                }
            }
        }
        return null
    }
    fun refresh(id: String) { revisions[id] = (revisions[id] ?: 0) + 1; cache.remove(id); missing.remove(id); changed.run() }
    fun close() { closed = true; worker.shutdownNow(); cache.evictAll(); missing.clear(); revisions.clear() }
}
