package com.sweaxstudios.sweaxdrop

import android.app.Application
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Intent
import android.net.wifi.WifiManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import org.json.JSONObject

/**
 * Starts the core once per process and routes its callbacks: replies and
 * events to the WebView, file access to [Storage], progress to the
 * foreground service, incoming offers to a notification.
 */
class App : Application(), Core.Host {
    lateinit var storage: Storage
    private val main = Handler(Looper.getMainLooper())
    private var multicast: WifiManager.MulticastLock? = null

    /** The visible UI, if any. Events reach it in order on the main thread. */
    @Volatile var ui: ((String) -> Unit)? = null
    @Volatile var replies: ((Int, String) -> Unit)? = null
    private val backlog = ArrayDeque<String>()

    @Volatile var started = false
        private set
    @Volatile var stayReady = false

    override fun onCreate() {
        super.onCreate()
        instance = this
        storage = Storage(this)
        channels()
        startCore()
    }

    fun startCore() {
        if (started) return
        val wifi = applicationContext.getSystemService(WIFI_SERVICE) as WifiManager
        multicast = wifi.createMulticastLock("sweaxdrop-discovery").apply {
            setReferenceCounted(false)
            acquire()
        }
        val name = Settings.Global.getString(contentResolver, Settings.Global.DEVICE_NAME)
            ?: Build.MODEL ?: "Android phone"
        val dir = filesDir.resolve("core").apply { mkdirs() }
        started = Core.start(dir.absolutePath, name, Build.MODEL ?: "", this)
    }

    private fun channels() {
        val nm = getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(NotificationChannel(CH_TRANSFER, getString(R.string.channel_transfers), NotificationManager.IMPORTANCE_LOW))
        nm.createNotificationChannel(NotificationChannel(CH_REQUEST, getString(R.string.channel_requests), NotificationManager.IMPORTANCE_HIGH))
        nm.createNotificationChannel(NotificationChannel(CH_READY, getString(R.string.channel_ready), NotificationManager.IMPORTANCE_MIN))
    }

    // ------------------------------------------------------------ Core.Host

    override fun onEvent(json: String) {
        TransferService.onCoreEvent(this, json)
        main.post {
            val sink = ui
            if (sink != null) sink(json) else {
                backlog.addLast(json)
                while (backlog.size > 200) backlog.removeFirst()
            }
        }
    }

    fun drainBacklog() {
        main.post {
            val sink = ui ?: return@post
            while (backlog.isNotEmpty()) sink(backlog.removeFirst())
        }
    }

    override fun onReply(id: Int, json: String) {
        main.post { replies?.invoke(id, json) }
    }

    override fun openRead(token: String): Int = storage.openRead(token)
    override fun createOutput(req: String): String = storage.createOutput(req)
    override fun finishOutput(handle: String, ok: Boolean, mtime: Long): String = storage.finishOutput(handle, ok, mtime)

    override fun setClipboard(text: String) {
        main.post {
            val cm = getSystemService(ClipboardManager::class.java)
            cm.setPrimaryClip(ClipData.newPlainText("SweaxDrop", text))
        }
    }

    override fun notify(json: String) {
        val o = JSONObject(json)
        if (ui != null) return // the app is open; the UI shows it
        val open = PendingIntent.getActivity(
            this, 0, Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val n = android.app.Notification.Builder(this, CH_REQUEST)
            .setSmallIcon(R.drawable.ic_stat)
            .setContentTitle(o.optString("title"))
            .setContentText(o.optString("body"))
            .setContentIntent(open)
            .setAutoCancel(true)
            .build()
        try {
            getSystemService(NotificationManager::class.java).notify(NOTE_REQUEST, n)
        } catch (_: SecurityException) {}
    }

    companion object {
        lateinit var instance: App
        const val CH_TRANSFER = "transfers"
        const val CH_REQUEST = "requests"
        const val CH_READY = "ready"
        const val NOTE_REQUEST = 2
    }
}
