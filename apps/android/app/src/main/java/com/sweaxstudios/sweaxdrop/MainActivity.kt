package com.sweaxstudios.sweaxdrop

import android.Manifest
import android.annotation.SuppressLint
import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.ClipboardManager
import android.content.Intent
import android.content.pm.PackageManager
import android.content.res.Configuration
import android.graphics.Color
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.provider.MediaStore
import android.view.View
import android.view.WindowInsets
import android.webkit.JavascriptInterface
import android.webkit.PermissionRequest
import android.webkit.ValueCallback
import android.webkit.WebChromeClient
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import org.json.JSONArray
import org.json.JSONObject

/**
 * One full-screen WebView with the shared SweaxDrop UI (bundled in assets),
 * talking to the core through `window.SweaxNative.post(json)`.
 */
class MainActivity : Activity() {
    private lateinit var web: WebView
    private val app get() = application as App
    private var pickId = 0
    private var pickKind = ""
    private var pendingCamera: PermissionRequest? = null

    @SuppressLint("SetJavaScriptEnabled")
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.setDecorFitsSystemWindows(false)
        window.statusBarColor = Color.TRANSPARENT
        window.navigationBarColor = Color.TRANSPARENT

        WebView.setWebContentsDebuggingEnabled(BuildConfigFlags.debuggable(this))
        web = WebView(this).apply {
            setBackgroundColor(Color.parseColor("#0C0E12"))
            settings.javaScriptEnabled = true
            settings.domStorageEnabled = true
            settings.mediaPlaybackRequiresUserGesture = false
            settings.allowFileAccess = false
            settings.allowContentAccess = false
            webViewClient = AssetClient()
            webChromeClient = object : WebChromeClient() {
                override fun onPermissionRequest(request: PermissionRequest) {
                    if (PermissionRequest.RESOURCE_VIDEO_CAPTURE in request.resources) {
                        if (checkSelfPermission(Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED) {
                            request.grant(arrayOf(PermissionRequest.RESOURCE_VIDEO_CAPTURE))
                        } else {
                            pendingCamera = request
                            requestPermissions(arrayOf(Manifest.permission.CAMERA), REQ_CAMERA)
                        }
                    } else request.deny()
                }
            }
            addJavascriptInterface(Bridge(), "SweaxNative")
        }
        // Safe-area insets for the CSS env() values the UI uses.
        web.setOnApplyWindowInsetsListener { v, insets ->
            val bars = insets.getInsets(WindowInsets.Type.systemBars() or WindowInsets.Type.ime())
            v.setPadding(0, 0, 0, 0)
            val d = resources.displayMetrics.density
            js("document.documentElement.style.setProperty('--sat','${bars.top / d}px');document.documentElement.style.setProperty('--sab','${bars.bottom / d}px')")
            insets
        }
        setContentView(web)
        app.ui = { json -> js("window.__swx&&window.__swx.event(${JSONObject.quote(json)})") }
        app.replies = { id, json -> js("window.__swx&&window.__swx.resolve($id,${JSONObject.quote(json)})") }
        // `adb shell am start ... --es demo live` opens the built-in demo
        // (pretend devices), used to record the README videos.
        val demo = intent.getStringExtra("demo")
        web.loadUrl(if (demo != null) "$ORIGIN/index.html?mock=1&mobile=1&demo=$demo&theme=${intent.getStringExtra("theme") ?: "dark"}&lang=${intent.getStringExtra("lang") ?: "en"}" else "$ORIGIN/index.html")
        if (Build.VERSION.SDK_INT >= 33 && checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
            requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), REQ_NOTIFY)
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        if (pageReady) handleShare(intent)
    }

    override fun onDestroy() {
        if (app.ui != null) {
            app.ui = null
            app.replies = null
        }
        web.destroy()
        super.onDestroy()
    }

    override fun onConfigurationChanged(newConfig: Configuration) {
        super.onConfigurationChanged(newConfig)
        js("window.dispatchEvent(new Event('resize'))")
    }

    @Deprecated("Deprecated in Java")
    override fun onBackPressed() {
        js("window.__swx&&window.__swx.back&&window.__swx.back()") { handled ->
            if (handled != "true") {
                @Suppress("DEPRECATION")
                super.onBackPressed()
            }
        }
    }

    private var pageReady = false

    private fun js(code: String, done: ((String) -> Unit)? = null) {
        runOnUiThread {
            if (::web.isInitialized) web.evaluateJavascript(code, done?.let { cb -> ValueCallback<String> { cb(it ?: "") } })
        }
    }

    private fun resolve(id: Int, value: JSONObject?) = js("window.__swx.resolve($id,${JSONObject.quote(value?.toString() ?: "null")})")
    private fun reject(id: Int, msg: String) = js("window.__swx.reject($id,${JSONObject.quote(msg)})")

    /** Shared from another app (Gallery, Files, a browser…): stage it in the UI. */
    private fun handleShare(intent: Intent?) {
        intent ?: return
        val uris = mutableListOf<Uri>()
        when (intent.action) {
            Intent.ACTION_SEND -> intent.getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java)?.let { uris += it }
            Intent.ACTION_SEND_MULTIPLE -> intent.getParcelableArrayListExtra(Intent.EXTRA_STREAM, Uri::class.java)?.let { uris += it }
            else -> return
        }
        val sources = JSONArray()
        Thread {
            for (u in uris) app.storage.describe(u)?.let { sources.put(it) }
            val text = intent.getStringExtra(Intent.EXTRA_TEXT)
            val action = JSONObject().put("type", "stage").put("sources", sources)
            if (sources.length() == 0 && !text.isNullOrBlank()) action.put("type", "text").put("text", text)
            js("window.__swx&&window.__swx.action(${JSONObject.quote(action.toString())})")
        }.start()
        setIntent(Intent(this, MainActivity::class.java))
    }

    // ------------------------------------------------------------ pickers

    private fun startPick(id: Int, kind: String) {
        pickId = id
        pickKind = kind
        val intent = when (kind) {
            "photos" -> Intent(MediaStore.ACTION_PICK_IMAGES).apply {
                type = "*/*"
                putExtra(Intent.EXTRA_MIME_TYPES, arrayOf("image/*", "video/*"))
                putExtra(MediaStore.EXTRA_PICK_IMAGES_MAX, MediaStore.getPickImagesMaxLimit())
            }
            "folder", "dir" -> Intent(Intent.ACTION_OPEN_DOCUMENT_TREE)
            else -> Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
                addCategory(Intent.CATEGORY_OPENABLE)
                type = "*/*"
                putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true)
            }
        }
        try {
            startActivityForResult(intent, REQ_PICK)
        } catch (e: ActivityNotFoundException) {
            if (kind == "photos") startPick(id, "files") else reject(id, "no picker")
        }
    }

    @Deprecated("Deprecated in Java")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        @Suppress("DEPRECATION")
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != REQ_PICK) return
        val id = pickId
        if (resultCode != RESULT_OK || data == null) {
            resolve(id, null)
            return
        }
        Thread {
            try {
                when (pickKind) {
                    "dir" -> {
                        val tree = data.data!!
                        contentResolver.takePersistableUriPermission(tree, Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
                        resolve(id, JSONObject().put("uri", tree.toString()).put("label", app.storage.treeLabel(tree)))
                    }
                    "folder" -> {
                        val tree = data.data!!
                        val out = JSONArray()
                        app.storage.walkTree(tree, out)
                        resolve(id, JSONObject().put("sources", out))
                    }
                    else -> {
                        val uris = mutableListOf<Uri>()
                        data.clipData?.let { c -> for (i in 0 until c.itemCount) uris += c.getItemAt(i).uri }
                        if (uris.isEmpty()) data.data?.let { uris += it }
                        val out = JSONArray()
                        for (u in uris) app.storage.describe(u)?.let { out.put(it) }
                        resolve(id, JSONObject().put("sources", out))
                    }
                }
            } catch (e: Exception) {
                reject(id, e.message ?: "could not read the selection")
            }
        }.start()
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode == REQ_CAMERA) {
            val r = pendingCamera ?: return
            pendingCamera = null
            if (grantResults.firstOrNull() == PackageManager.PERMISSION_GRANTED) r.grant(arrayOf(PermissionRequest.RESOURCE_VIDEO_CAPTURE)) else r.deny()
        }
    }

    // ------------------------------------------------------------ JS bridge

    inner class Bridge {
        @JavascriptInterface
        fun post(message: String) {
            val m = JSONObject(message)
            val id = m.optInt("id")
            val op = m.optString("op")
            val args = m.optJSONObject("args") ?: JSONObject()
            when (op) {
                "call" -> {
                    val req = args.optString("req")
                    if (req.contains("\"stay_ready\"")) {
                        val on = JSONObject(req).optJSONObject("patch")?.optBoolean("stay_ready")
                        if (on != null) TransferService.setStayReady(this@MainActivity, on)
                    }
                    Core.call(id, req)
                }
                "ready" -> runOnUiThread {
                    pageReady = true
                    app.drainBacklog()
                    handleShare(intent)
                    resolve(id, JSONObject().put("ok", true))
                }
                "pick" -> runOnUiThread { startPick(id, args.optString("what")) }
                "pickDir" -> runOnUiThread { startPick(id, "dir") }
                "clipboardRead" -> runOnUiThread {
                    val cm = getSystemService(ClipboardManager::class.java)
                    val t = cm.primaryClip?.takeIf { it.itemCount > 0 }?.getItemAt(0)?.coerceToText(this@MainActivity)?.toString() ?: ""
                    resolve(id, JSONObject().put("text", t))
                }
                "clipboardWrite" -> {
                    app.setClipboard(args.optString("text"))
                    resolve(id, JSONObject())
                }
                "open", "openUrl" -> runOnUiThread {
                    val target = args.optString("path").ifEmpty { args.optString("url") }
                    try {
                        val u = Uri.parse(target)
                        val i = Intent(Intent.ACTION_VIEW, u).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                        if (u.scheme == "content") i.setDataAndType(u, contentResolver.getType(u))
                        startActivity(i)
                    } catch (_: Exception) {}
                    resolve(id, JSONObject())
                }
                else -> reject(id, "unknown op $op")
            }
        }
    }

    /** Serves the bundled UI from a fixed https origin (a secure context, so the camera works for QR scanning). */
    private inner class AssetClient : WebViewClient() {
        override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest): WebResourceResponse? {
            val url = request.url
            if (url.host != HOST) return null
            val path = (url.path ?: "/").trimStart('/').ifEmpty { "index.html" }
            return try {
                val stream = assets.open("ui/$path")
                val mime = when (path.substringAfterLast('.')) {
                    "html" -> "text/html"
                    "js" -> "text/javascript"
                    "css" -> "text/css"
                    "svg" -> "image/svg+xml"
                    "woff2" -> "font/woff2"
                    "png" -> "image/png"
                    "json" -> "application/json"
                    else -> "application/octet-stream"
                }
                WebResourceResponse(mime, if (mime.startsWith("text") || mime.endsWith("json")) "utf-8" else null, stream)
            } catch (e: Exception) {
                WebResourceResponse("text/plain", "utf-8", 404, "Not Found", null, null)
            }
        }

        override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
            if (request.url.host == HOST) return false
            try {
                startActivity(Intent(Intent.ACTION_VIEW, request.url))
            } catch (_: Exception) {}
            return true
        }
    }

    companion object {
        const val HOST = "appassets.androidplatform.net"
        const val ORIGIN = "https://$HOST"
        private const val REQ_PICK = 10
        private const val REQ_CAMERA = 11
        private const val REQ_NOTIFY = 12
    }
}

object BuildConfigFlags {
    fun debuggable(a: Activity) = (a.applicationInfo.flags and android.content.pm.ApplicationInfo.FLAG_DEBUGGABLE) != 0
}

@Suppress("unused")
private fun View.noop() = Unit
