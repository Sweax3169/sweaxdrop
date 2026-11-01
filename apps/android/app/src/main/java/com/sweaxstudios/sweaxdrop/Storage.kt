package com.sweaxstudios.sweaxdrop

import android.content.ContentValues
import android.content.Context
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.os.ParcelFileDescriptor
import android.provider.DocumentsContract
import android.provider.MediaStore
import android.provider.OpenableColumns
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/**
 * Where received files go and where sent files come from.
 *
 * Received: photos and videos land in Pictures/SweaxDrop and Movies/SweaxDrop
 * (so the gallery shows them), everything else in Download/SweaxDrop, keeping
 * folder structure. If the user picked a folder (SAF tree), files go there.
 * Files are written while marked pending, so half-received files never show
 * up in the gallery.
 */
class Storage(private val ctx: Context) {
    private val resolver = ctx.contentResolver

    fun openRead(token: String): Int = try {
        resolver.openFileDescriptor(Uri.parse(token), "r")?.detachFd() ?: -1
    } catch (e: Exception) {
        -1
    }

    fun createOutput(reqJson: String): String {
        val req = JSONObject(reqJson)
        val path = req.optString("path").trimStart('/')
        val mime = req.optString("mime").takeIf { it.isNotEmpty() && it != "null" } ?: guessMime(path)
        val dest = req.optString("dest")
        val gallery = req.optBoolean("gallery", true)
        val name = path.substringAfterLast('/')
        val sub = path.substringBeforeLast('/', "")
        // Small files skip the media provider: three provider calls per file
        // made a thousand small files take half a minute.
        if (dest.isEmpty() && Build.VERSION.SDK_INT >= 30 && req.optLong("size", Long.MAX_VALUE) <= DIRECT_MAX) {
            createDirect(sub, name, mime, gallery)?.let { return it }
        }
        return try {
            val uri = (if (dest.isNotEmpty()) createInTree(Uri.parse(dest), sub, name, mime) else createInMediaStore(sub, name, mime, gallery))
                ?: return ""
            val fd = resolver.openFileDescriptor(uri, "rw")?.detachFd() ?: return ""
            JSONObject().put("fd", fd).put("handle", uri.toString()).toString()
        } catch (e: Exception) {
            ""
        }
    }

    /** A file created by path in shared storage (Android 11+, no permission needed for our own files). */
    private fun createDirect(sub: String, name: String, mime: String, gallery: Boolean): String? = try {
        val base = when {
            gallery && mime.startsWith("image/") -> Environment.DIRECTORY_PICTURES
            gallery && mime.startsWith("video/") -> Environment.DIRECTORY_MOVIES
            else -> Environment.DIRECTORY_DOWNLOADS
        }
        val dir = File(Environment.getExternalStoragePublicDirectory(base), listOf("SweaxDrop", sub).filter { it.isNotEmpty() }.joinToString("/"))
        if (!dir.isDirectory && !dir.mkdirs() && !dir.isDirectory) null
        else {
            val f = uniqueFile(dir, name)
            val fd = ParcelFileDescriptor.open(f, ParcelFileDescriptor.MODE_READ_WRITE).detachFd()
            JSONObject().put("fd", fd).put("handle", Uri.fromFile(f).toString()).toString()
        }
    } catch (e: Exception) {
        null
    }

    /** name, then "name (1)", "name (2)"...; created atomically, so parallel writers never share a file. */
    private fun uniqueFile(dir: File, name: String): File {
        val dot = name.lastIndexOf('.').takeIf { it > 0 } ?: name.length
        val stem = name.substring(0, dot)
        val ext = name.substring(dot)
        var f = File(dir, name)
        var i = 1
        while (!f.createNewFile()) {
            f = File(dir, "$stem ($i)$ext")
            if (++i > 9999) throw java.io.IOException("no free name for $name")
        }
        return f
    }

    private fun createInMediaStore(sub: String, name: String, mime: String, gallery: Boolean): Uri? {
        val isImage = mime.startsWith("image/")
        val isVideo = mime.startsWith("video/")
        val (collection, base) = when {
            gallery && isImage -> MediaStore.Images.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY) to Environment.DIRECTORY_PICTURES
            gallery && isVideo -> MediaStore.Video.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY) to Environment.DIRECTORY_MOVIES
            else -> MediaStore.Downloads.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY) to Environment.DIRECTORY_DOWNLOADS
        }
        val rel = listOf(base, "SweaxDrop", sub).filter { it.isNotEmpty() }.joinToString("/") + "/"
        val values = ContentValues().apply {
            put(MediaStore.MediaColumns.DISPLAY_NAME, name)
            put(MediaStore.MediaColumns.MIME_TYPE, mime)
            put(MediaStore.MediaColumns.RELATIVE_PATH, rel)
            put(MediaStore.MediaColumns.IS_PENDING, 1)
        }
        return resolver.insert(collection, values)
    }

    private fun createInTree(tree: Uri, sub: String, name: String, mime: String): Uri? {
        var dir = DocumentsContract.buildDocumentUriUsingTree(tree, DocumentsContract.getTreeDocumentId(tree))
        for (part in sub.split('/').filter { it.isNotEmpty() }) {
            dir = findChild(tree, dir, part) ?: DocumentsContract.createDocument(resolver, dir, DocumentsContract.Document.MIME_TYPE_DIR, part) ?: return null
        }
        return DocumentsContract.createDocument(resolver, dir, mime, name)
    }

    private fun findChild(tree: Uri, parent: Uri, name: String): Uri? {
        val children = DocumentsContract.buildChildDocumentsUriUsingTree(tree, DocumentsContract.getDocumentId(parent))
        resolver.query(children, arrayOf(DocumentsContract.Document.COLUMN_DOCUMENT_ID, DocumentsContract.Document.COLUMN_DISPLAY_NAME), null, null, null)?.use { c ->
            while (c.moveToNext()) {
                if (c.getString(1) == name) return DocumentsContract.buildDocumentUriUsingTree(tree, c.getString(0))
            }
        }
        return null
    }

    fun finishOutput(handle: String, ok: Boolean, mtime: Long): String {
        val uri = Uri.parse(handle)
        if (uri.scheme == "file") {
            val f = File(uri.path ?: return "")
            if (!ok) {
                f.delete()
                return ""
            }
            if (mtime > 0) f.setLastModified(mtime)
            return handle
        }
        return try {
            if (!ok) {
                try {
                    if (uri.authority == MediaStore.AUTHORITY) resolver.delete(uri, null, null) else DocumentsContract.deleteDocument(resolver, uri)
                } catch (_: Exception) {}
                ""
            } else {
                if (uri.authority == MediaStore.AUTHORITY) {
                    val v = ContentValues().apply {
                        put(MediaStore.MediaColumns.IS_PENDING, 0)
                        if (mtime > 0) put(MediaStore.MediaColumns.DATE_MODIFIED, mtime / 1000)
                    }
                    resolver.update(uri, v, null, null)
                }
                handle
            }
        } catch (e: Exception) {
            ""
        }
    }

    /** One picked document → a source entry for the core. */
    fun describe(uri: Uri, relPath: String? = null): JSONObject? {
        var name = relPath ?: "file"
        var size = -1L
        var mtime = 0L
        try {
            resolver.query(uri, null, null, null, null)?.use { c ->
                if (c.moveToFirst()) {
                    val n = c.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                    val s = c.getColumnIndex(OpenableColumns.SIZE)
                    val m = c.getColumnIndex(DocumentsContract.Document.COLUMN_LAST_MODIFIED)
                    if (relPath == null && n >= 0) name = c.getString(n) ?: name
                    if (s >= 0 && !c.isNull(s)) size = c.getLong(s)
                    if (m >= 0 && !c.isNull(m)) mtime = c.getLong(m)
                    // The photo picker hides real names behind the media id ("1000179895.mp4").
                    if (relPath == null && PICKER_NAME.matches(name)) {
                        val t = c.getColumnIndex(MediaStore.MediaColumns.DATE_TAKEN)
                        val taken = if (t >= 0 && !c.isNull(t)) c.getLong(t) else mtime
                        name = cameraName(name, resolver.getType(uri) ?: guessMime(name), taken)
                    }
                }
            }
        } catch (_: Exception) {}
        if (size < 0) {
            size = try {
                resolver.openFileDescriptor(uri, "r")?.use { it.statSize } ?: -1
            } catch (_: Exception) {
                -1
            }
        }
        if (size < 0) return null
        return JSONObject()
            .put("token", uri.toString())
            .put("path", name)
            .put("size", size)
            .put("mime", resolver.getType(uri) ?: guessMime(name))
            .apply { if (mtime > 0) put("mtime", mtime) }
    }

    /** Every file under a picked folder, with paths starting at the folder's name. */
    fun walkTree(tree: Uri, out: JSONArray) {
        val rootId = DocumentsContract.getTreeDocumentId(tree)
        val rootName = rootId.substringAfterLast(':').substringAfterLast('/').ifEmpty { "Folder" }
        walk(tree, rootId, rootName, out)
    }

    private fun walk(tree: Uri, docId: String, rel: String, out: JSONArray) {
        val children = DocumentsContract.buildChildDocumentsUriUsingTree(tree, docId)
        val cols = arrayOf(
            DocumentsContract.Document.COLUMN_DOCUMENT_ID,
            DocumentsContract.Document.COLUMN_DISPLAY_NAME,
            DocumentsContract.Document.COLUMN_MIME_TYPE,
            DocumentsContract.Document.COLUMN_SIZE,
            DocumentsContract.Document.COLUMN_LAST_MODIFIED,
        )
        resolver.query(children, cols, null, null, null)?.use { c ->
            while (c.moveToNext()) {
                val id = c.getString(0)
                val name = c.getString(1) ?: continue
                val mime = c.getString(2) ?: ""
                if (mime == DocumentsContract.Document.MIME_TYPE_DIR) {
                    walk(tree, id, "$rel/$name", out)
                } else {
                    val uri = DocumentsContract.buildDocumentUriUsingTree(tree, id)
                    out.put(
                        JSONObject().put("token", uri.toString()).put("path", "$rel/$name").put("size", c.getLong(3)).put("mime", mime)
                            .apply { if (!c.isNull(4)) put("mtime", c.getLong(4)) },
                    )
                }
            }
        }
    }

    fun treeLabel(tree: Uri): String {
        val id = DocumentsContract.getTreeDocumentId(tree)
        val vol = id.substringBefore(':')
        val rest = id.substringAfter(':', "")
        return (if (vol == "primary") "" else "$vol/") + rest.ifEmpty { "/" }
    }

    companion object {
        /** Files up to this size are written by path instead of through the media provider. */
        const val DIRECT_MAX = 4L shl 20

        private val PICKER_NAME = Regex("""^\d+(\.[A-Za-z0-9]+)?$""")

        /** "IMG_20261102_013455.jpg" / "VID_….mp4", like the camera names them. */
        fun cameraName(picked: String, mime: String, takenMs: Long): String {
            val ext = picked.substringAfterLast('.', "").ifEmpty { mime.substringAfter('/', "bin") }
            val prefix = if (mime.startsWith("video/")) "VID" else "IMG"
            val stamp = if (takenMs > 0) SimpleDateFormat("yyyyMMdd_HHmmss", Locale.US).format(Date(takenMs)) else picked.substringBefore('.')
            return "${prefix}_$stamp.$ext"
        }

        fun guessMime(name: String): String {
            val ext = name.substringAfterLast('.', "").lowercase()
            return android.webkit.MimeTypeMap.getSingleton().getMimeTypeFromExtension(ext) ?: "application/octet-stream"
        }

        @Suppress("unused")
        fun closeQuietly(p: ParcelFileDescriptor?) = try { p?.close() } catch (_: Exception) {}
    }
}
