package com.sweaxstudios.sweaxdrop

import android.app.Notification
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.IBinder
import android.os.PowerManager
import org.json.JSONObject

/**
 * Keeps the process alive while files move (Android stops background
 * networking otherwise) and shows one progress notification. With
 * "Stay ready in the background" on, it also keeps running when idle so the
 * phone can receive while the app is closed.
 */
class TransferService : Service() {
    private var wake: PowerManager.WakeLock? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        wake = getSystemService(PowerManager::class.java).newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "sweaxdrop:transfer").apply {
            setReferenceCounted(false)
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val n = build(this)
        startForeground(NOTE_ID, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
        if (active.isNotEmpty()) wake?.acquire(30 * 60 * 1000L) else wake?.release()
        return START_NOT_STICKY
    }

    override fun onDestroy() {
        wake?.release()
        super.onDestroy()
    }

    companion object {
        private const val NOTE_ID = 1
        private val active = LinkedHashMap<String, JSONObject>()
        private var running = false
        private var lastUpdate = 0L

        private fun build(ctx: Context): Notification {
            val open = PendingIntent.getActivity(
                ctx, 1, Intent(ctx, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
            val b = Notification.Builder(ctx, if (active.isEmpty()) App.CH_READY else App.CH_TRANSFER)
                .setSmallIcon(R.drawable.ic_stat)
                .setContentIntent(open)
                .setOngoing(true)
                .setOnlyAlertOnce(true)
            if (active.isEmpty()) {
                b.setContentTitle(ctx.getString(R.string.ready))
            } else {
                val t = active.values.first()
                val send = t.optString("dir") == "send"
                val peer = t.optJSONObject("peer")?.optString("name") ?: ""
                val total = t.optLong("total")
                val bytes = t.optLong("bytes")
                val pct = if (total > 0) (bytes * 100 / total).toInt() else 0
                val speed = t.optDouble("speed") / 1e6
                b.setContentTitle("${ctx.getString(if (send) R.string.sending else R.string.receiving)} · $peer")
                    .setContentText(String.format("%d%% · %.1f MB/s", pct, speed))
                    .setProgress(100, pct, total == 0L)
            }
            return b.build()
        }

        /** Called for every core event, on a core thread. */
        fun onCoreEvent(ctx: Context, json: String) {
            if (!json.contains("\"type\":\"transfer\"")) return
            val t = JSONObject(json).optJSONObject("transfer") ?: return
            val id = t.optString("id")
            val phase = t.optString("phase")
            synchronized(active) {
                if (phase in setOf("running", "paused", "connecting", "waiting", "reconnecting")) active[id] = t else active.remove(id)
                val now = System.currentTimeMillis()
                val want = active.isNotEmpty() || App.instance.stayReady
                when {
                    want && !running -> {
                        running = true
                        ctx.startForegroundService(Intent(ctx, TransferService::class.java))
                    }
                    !want && running -> {
                        running = false
                        ctx.stopService(Intent(ctx, TransferService::class.java))
                    }
                    running && now - lastUpdate > 900 -> {
                        lastUpdate = now
                        try {
                            ctx.getSystemService(NotificationManager::class.java).notify(NOTE_ID, build(ctx))
                        } catch (_: SecurityException) {}
                    }
                }
            }
        }

        fun setStayReady(ctx: Context, on: Boolean) {
            App.instance.stayReady = on
            synchronized(active) {
                if (on && !running) {
                    running = true
                    ctx.startForegroundService(Intent(ctx, TransferService::class.java))
                } else if (!on && running && active.isEmpty()) {
                    running = false
                    ctx.stopService(Intent(ctx, TransferService::class.java))
                }
            }
        }
    }
}
