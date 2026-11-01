import Foundation
import UIKit
import UserNotifications

/// Owns the Rust core (libsweaxdrop.a) and turns its C callbacks into Swift.
/// Replies and events go to the web view; file access goes to `Storage`.
final class CoreBridge {
    static let shared = CoreBridge()

    var onEvent: ((String) -> Void)?
    var onReply: ((Int32, String) -> Void)?
    let storage = Storage()
    private(set) var started = false
    private var backlog: [String] = []

    func start() {
        guard !started else { return }
        let docs = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("core")
        try? FileManager.default.createDirectory(at: docs, withIntermediateDirectories: true)
        let ctx = Unmanaged.passUnretained(self).toOpaque()
        let cb = SdCallbacks(
            ctx: ctx,
            on_event: { ctx, json in
                guard let ctx, let json else { return }
                let me = Unmanaged<CoreBridge>.fromOpaque(ctx).takeUnretainedValue()
                me.event(String(cString: json))
            },
            on_reply: { ctx, id, json in
                guard let ctx, let json else { return }
                let me = Unmanaged<CoreBridge>.fromOpaque(ctx).takeUnretainedValue()
                let s = String(cString: json)
                DispatchQueue.main.async { me.onReply?(id, s) }
            },
            open_read: { ctx, token in
                guard let ctx, let token else { return -1 }
                let me = Unmanaged<CoreBridge>.fromOpaque(ctx).takeUnretainedValue()
                return Int64(me.storage.openRead(String(cString: token)))
            },
            create_output: { ctx, req, out, cap in
                guard let ctx, let req, let out else { return -1 }
                let me = Unmanaged<CoreBridge>.fromOpaque(ctx).takeUnretainedValue()
                guard let answer = me.storage.createOutput(String(cString: req)) else { return -1 }
                return CoreBridge.write(answer, out, cap)
            },
            finish_output: { ctx, handle, ok, mtime, out, cap in
                guard let ctx, let handle, let out else { return -1 }
                let me = Unmanaged<CoreBridge>.fromOpaque(ctx).takeUnretainedValue()
                let answer = me.storage.finishOutput(String(cString: handle), ok: ok, mtime: mtime)
                return CoreBridge.write(answer, out, cap)
            },
            set_clipboard: { _, text in
                guard let text else { return }
                let s = String(cString: text)
                DispatchQueue.main.async { UIPasteboard.general.string = s }
            },
            notify: { _, title, body in
                let t = title.map { String(cString: $0) } ?? "SweaxDrop"
                let b = body.map { String(cString: $0) } ?? ""
                DispatchQueue.main.async {
                    guard UIApplication.shared.applicationState != .active else { return }
                    let c = UNMutableNotificationContent()
                    c.title = t
                    c.body = b
                    c.sound = .default
                    UNUserNotificationCenter.current().add(UNNotificationRequest(identifier: UUID().uuidString, content: c, trigger: nil))
                }
            }
        )
        let name = UIDevice.current.name
        let model = UIDevice.current.model
        // iOS blocks raw multicast without Apple's entitlement, so the core's
        // UDP discovery stays on only for answering; Bonjour (Discovery.swift)
        // finds and announces devices instead.
        started = docs.path.withCString { d in
            name.withCString { n in
                model.withCString { m in
                    "ios".withCString { p in sd_start(d, n, m, p, true, cb) }
                }
            }
        }
    }

    private static func write(_ s: String, _ out: UnsafeMutablePointer<CChar>, _ cap: Int32) -> Int32 {
        let bytes = Array(s.utf8)
        guard bytes.count < Int(cap) else { return -1 }
        bytes.withUnsafeBufferPointer { buf in
            out.withMemoryRebound(to: UInt8.self, capacity: Int(cap)) { dst in
                dst.update(from: buf.baseAddress!, count: bytes.count)
            }
        }
        return Int32(bytes.count)
    }

    private func event(_ json: String) {
        TransferActivity.shared.observe(json)
        DispatchQueue.main.async {
            if let sink = self.onEvent {
                sink(json)
            } else {
                self.backlog.append(json)
                if self.backlog.count > 200 { self.backlog.removeFirst() }
            }
        }
    }

    func drainBacklog() {
        guard let sink = onEvent else { return }
        let items = backlog
        backlog.removeAll()
        items.forEach(sink)
    }

    func call(id: Int32, request: String) {
        request.withCString { sd_call(id, $0) }
    }

    var port: UInt16 { sd_port() }
    var deviceId: String { sd_device_id().map { String(cString: $0) } ?? "" }
}

/// Keeps the app running briefly in the background while files move
/// (iOS suspends network work in the background otherwise).
final class TransferActivity {
    static let shared = TransferActivity()
    private var active = Set<String>()
    private var task: UIBackgroundTaskIdentifier = .invalid

    func observe(_ json: String) {
        guard json.contains("\"type\":\"transfer\""),
              let data = json.data(using: .utf8),
              let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let t = obj["transfer"] as? [String: Any],
              let id = t["id"] as? String,
              let phase = t["phase"] as? String else { return }
        DispatchQueue.main.async {
            if ["running", "paused", "connecting", "waiting", "reconnecting"].contains(phase) {
                self.active.insert(id)
            } else {
                self.active.remove(id)
            }
            self.update()
        }
    }

    private func update() {
        if !active.isEmpty && task == .invalid {
            task = UIApplication.shared.beginBackgroundTask(withName: "SweaxDrop transfer") { [weak self] in
                guard let self else { return }
                UIApplication.shared.endBackgroundTask(self.task)
                self.task = .invalid
            }
        } else if active.isEmpty && task != .invalid {
            UIApplication.shared.endBackgroundTask(task)
            task = .invalid
        }
    }
}
