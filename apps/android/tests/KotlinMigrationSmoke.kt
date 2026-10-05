package xin.soymilk.readall

import java.lang.reflect.Modifier
import java.nio.ByteBuffer
import java.nio.file.Files
import java.util.Base64

/** Frozen Java-written storage bytes and native descriptors, not Kotlin-generated expectations. */
object KotlinMigrationSmoke {
    private const val JAVA_INDEX = "UlNMMQAAAAEARTAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMmEuZXB1YgAU5Y6f5paH5Lu27aC97biALmVwdWIACeWOn+S5puWQjQAG5L2c6ICFAARFUFVCABRjb250ZW50Oi8vZml4dHVyZS80MgAP6Ieq5a6a5LmJ7aC97biAAAABi8/laAAAAAGLz+Vs0kBFQAAAAAAAAQ=="
    private fun descriptor(type: Class<*>): String = when (type) {
        java.lang.Void.TYPE -> "V"; java.lang.Boolean.TYPE -> "Z"; java.lang.Integer.TYPE -> "I"; java.lang.Long.TYPE -> "J"
        else -> if (type.isArray) type.name.replace('.', '/') else "L${type.name.replace('.', '/')};"
    }
    @JvmStatic fun main(args: Array<String>) {
        val expected = mapOf(
            "nativeOpen" to "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;IIIIII)J",
            "nativeViewport" to "(JIIII)V", "nativeState" to "(J)[Ljava/lang/String;", "nativeContents" to "(J)[Ljava/lang/String;",
            "nativeCommand" to "(JIII)V", "nativeCopyPixels" to "(JJLjava/nio/ByteBuffer;)Z", "nativeClose" to "(J)V",
            "nativeInput" to "(JLjava/lang/String;Ljava/lang/String;)V", "nativeHostReply" to "(JILjava/lang/String;)V",
            "nativeEffects" to "(J)[Ljava/lang/String;", "nativeAppearance" to "(Ljava/lang/String;Ljava/lang/String;)[Ljava/lang/String;",
            "nativePreview" to "(Ljava/lang/String;Ljava/nio/ByteBuffer;)[Ljava/lang/String;"
        )
        val native = NativeReader::class.java.declaredMethods.filter { Modifier.isNative(it.modifiers) }
        check(native.size == 12)
        check(native.associate { check(Modifier.isStatic(it.modifiers) && Modifier.isPrivate(it.modifiers)); it.name to "(${it.parameterTypes.joinToString("") { p -> descriptor(p) }})${descriptor(it.returnType)}" } == expected)
        for (method in NativeReader::class.java.declaredMethods.filter { it.name in setOf("state", "command", "checkedHandle", "contents", "viewport", "input", "hostReply", "effects", "close") }) check(Modifier.isSynchronized(method.modifiers))
        check(!Modifier.isSynchronized(NativeReader::class.java.getMethod("copyPixels", NativeReader.State::class.java, ByteBuffer::class.java).modifiers))
        check(NativeReader.NEXT == 1 && NativeReader.RESIZE == 12 && NativeReader.FIND == 20 && NativeReader.BACKSPACE == 31 && NativeReader.TOUCH == 40)
        val root = Files.createTempDirectory("readall-java-kotlin-storage-")
        try {
            val bytes = Base64.getDecoder().decode(JAVA_INDEX); val path = root.resolve("shelf-v1.bin"); Files.write(path, bytes)
            val store = ShelfStore(root.toFile()); val book = store.load().single()
            check(book.name == "原文件😀.epub" && book.title == "原书名" && book.author == "作者" && book.alias == "自定义😀" && book.pinned && book.percent == 42.5)
            check(book.added == 1700000000000L && book.opened == 1700000001234L && book.uri == "content://fixture/42")
            check(Files.readAllBytes(path).contentEquals(bytes)) // Reading does not rewrite old indices.
            store.rename(book.id, book.alias); check(Files.readAllBytes(path).contentEquals(bytes)) // The new writer preserves every byte, including modified UTF-8.
            store.progress(book.id, 50.0, 1700000005678L); val updated = ShelfStore(root.toFile()).load().single(); check(updated.percent == 50.0 && updated.alias == book.alias && updated.pinned)
        } finally { Files.walk(root).use { paths -> paths.sorted(Comparator.reverseOrder()).forEach { Files.deleteIfExists(it) } } }
        println("PASS Kotlin migration: all 12 static JNI descriptors, synchronization boundary, stable opcodes and byte-exact Java bookshelf compatibility")
    }
}
