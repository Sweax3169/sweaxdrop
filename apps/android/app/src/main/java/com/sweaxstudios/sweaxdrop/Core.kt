package com.sweaxstudios.sweaxdrop

/**
 * The Rust core (libsweaxdrop.so). Everything goes through JSON: [call]
 * sends a command, the answer comes back on [Host.onReply]; events arrive
 * on [Host.onEvent]. The other Host methods are file access the core asks
 * the app for, because on Android only the app may open content URIs and
 * write to MediaStore.
 */
object Core {
    init {
        System.loadLibrary("sweaxdrop")
    }

    @JvmStatic external fun start(dataDir: String, name: String, model: String, host: Host): Boolean
    @JvmStatic external fun call(id: Int, req: String)
    @JvmStatic external fun port(): Int

    interface Host {
        fun onEvent(json: String)
        fun onReply(id: Int, json: String)
        /** Opens a content URI for reading and hands the descriptor over (-1 on failure). */
        fun openRead(token: String): Int
        /** Creates an output file; returns {"fd": n, "handle": "..."} or "" on failure. */
        fun createOutput(req: String): String
        /** Publishes (ok) or deletes the output; returns where it ended up. */
        fun finishOutput(handle: String, ok: Boolean, mtime: Long): String
        fun setClipboard(text: String)
        fun notify(json: String)
    }
}
