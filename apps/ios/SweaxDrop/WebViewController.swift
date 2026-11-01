import PhotosUI
import UIKit
import UniformTypeIdentifiers
import WebKit

/// The shared SweaxDrop UI (bundled in `ui/`), served from `sweaxdrop://app/`
/// and talking to the core through `window.webkit.messageHandlers.swx`.
final class WebViewController: UIViewController, WKScriptMessageHandler, WKNavigationDelegate, PHPickerViewControllerDelegate, UIDocumentPickerDelegate {
    private var web: WKWebView!
    private var pickId: Int = 0
    private var pickKind = ""
    private var pageReady = false
    private var pendingActions: [String] = []

    override func loadView() {
        let cfg = WKWebViewConfiguration()
        cfg.setURLSchemeHandler(AssetHandler(), forURLScheme: "sweaxdrop")
        cfg.userContentController.add(WeakHandler(self), name: "swx")
        cfg.allowsInlineMediaPlayback = true
        cfg.mediaTypesRequiringUserActionForPlayback = []
        web = WKWebView(frame: .zero, configuration: cfg)
        web.isOpaque = false
        web.backgroundColor = UIColor(red: 0.047, green: 0.055, blue: 0.071, alpha: 1)
        web.scrollView.backgroundColor = web.backgroundColor
        web.scrollView.contentInsetAdjustmentBehavior = .never
        web.scrollView.bounces = false
        web.navigationDelegate = self
        if #available(iOS 16.4, *) { web.isInspectable = true }
        view = web
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        let core = CoreBridge.shared
        core.onEvent = { [weak self] json in self?.js("window.__swx&&window.__swx.event(\(Self.quote(json)))") }
        core.onReply = { [weak self] id, json in self?.js("window.__swx&&window.__swx.resolve(\(id),\(Self.quote(json)))") }
        web.load(URLRequest(url: URL(string: "sweaxdrop://app/index.html")!))
    }

    private func js(_ code: String) {
        web.evaluateJavaScript(code, completionHandler: nil)
    }

    private func resolve(_ id: Int, _ obj: Any?) {
        var s = "null"
        if let obj, let d = try? JSONSerialization.data(withJSONObject: obj), let str = String(data: d, encoding: .utf8) { s = str }
        js("window.__swx.resolve(\(id),\(Self.quote(s)))")
    }

    private func reject(_ id: Int, _ msg: String) {
        js("window.__swx.reject(\(id),\(Self.quote(msg)))")
    }

    static func quote(_ s: String) -> String {
        let d = try? JSONSerialization.data(withJSONObject: [s])
        let arr = d.flatMap { String(data: $0, encoding: .utf8) } ?? "[\"\"]"
        return String(arr.dropFirst().dropLast())
    }

    /// Files opened with SweaxDrop from another app ("Open in…" / Share).
    func stageIncoming(_ urls: [URL]) {
        let st = CoreBridge.shared.storage
        var sources: [[String: Any]] = []
        for u in urls {
            st.keepAccess(u)
            if let d = st.describe(u) { sources.append(d) }
        }
        let action: [String: Any] = ["type": "stage", "sources": sources]
        guard let d = try? JSONSerialization.data(withJSONObject: action), let s = String(data: d, encoding: .utf8) else { return }
        if pageReady { js("window.__swx&&window.__swx.action(\(Self.quote(s)))") } else { pendingActions.append(s) }
    }

    // MARK: messages from the page

    func userContentController(_ uc: WKUserContentController, didReceive message: WKScriptMessage) {
        guard let body = message.body as? String, let data = body.data(using: .utf8),
              let m = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return }
        let id = m["id"] as? Int ?? 0
        let op = m["op"] as? String ?? ""
        let args = m["args"] as? [String: Any] ?? [:]
        switch op {
        case "call":
            CoreBridge.shared.call(id: Int32(id), request: args["req"] as? String ?? "{}")
        case "ready":
            pageReady = true
            CoreBridge.shared.drainBacklog()
            for a in pendingActions { js("window.__swx&&window.__swx.action(\(Self.quote(a)))") }
            pendingActions.removeAll()
            resolve(id, ["ok": true])
        case "pick":
            pick(id: id, what: args["what"] as? String ?? "files")
        case "pickDir":
            pickId = id
            pickKind = "dir"
            let p = UIDocumentPickerViewController(forOpeningContentTypes: [.folder])
            p.delegate = self
            present(p, animated: true)
        case "clipboardRead":
            resolve(id, ["text": UIPasteboard.general.string ?? ""])
        case "clipboardWrite":
            UIPasteboard.general.string = args["text"] as? String ?? ""
            resolve(id, [:])
        case "open", "openUrl":
            let target = (args["path"] as? String).flatMap { $0.isEmpty ? nil : $0 } ?? (args["url"] as? String ?? "")
            if target == "Photos", let u = URL(string: "photos-redirect://") {
                UIApplication.shared.open(u)
            } else if let u = URL(string: target), u.scheme != nil, u.scheme != "file" {
                UIApplication.shared.open(u)
            } else {
                // Files app at our folder.
                let path = target.hasPrefix("/") ? target : CoreBridge.shared.storage.documents.path
                if let u = URL(string: "shareddocuments://" + path) { UIApplication.shared.open(u) }
            }
            resolve(id, [:])
        default:
            reject(id, "unknown op \(op)")
        }
    }

    // MARK: pickers

    private func pick(id: Int, what: String) {
        pickId = id
        pickKind = what
        switch what {
        case "photos":
            var cfg = PHPickerConfiguration(photoLibrary: .shared())
            cfg.filter = .any(of: [.images, .videos])
            cfg.selectionLimit = 0
            cfg.preferredAssetRepresentationMode = .current
            let p = PHPickerViewController(configuration: cfg)
            p.delegate = self
            present(p, animated: true)
        case "folder":
            let p = UIDocumentPickerViewController(forOpeningContentTypes: [.folder])
            p.delegate = self
            present(p, animated: true)
        default:
            let p = UIDocumentPickerViewController(forOpeningContentTypes: [.item], asCopy: false)
            p.allowsMultipleSelection = true
            p.delegate = self
            present(p, animated: true)
        }
    }

    func picker(_ picker: PHPickerViewController, didFinishPicking results: [PHPickerResult]) {
        picker.dismiss(animated: true)
        let id = pickId
        guard !results.isEmpty else { resolve(id, nil); return }
        let group = DispatchGroup()
        let lock = NSLock()
        var sources: [[String: Any]] = []
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("picked/\(UUID().uuidString)", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        for r in results {
            let provider = r.itemProvider
            let type = provider.registeredTypeIdentifiers.first { UTType($0)?.conforms(to: .movie) == true }
                ?? provider.registeredTypeIdentifiers.first { UTType($0)?.conforms(to: .image) == true }
            guard let type else { continue }
            group.enter()
            provider.loadFileRepresentation(forTypeIdentifier: type) { url, _ in
                defer { group.leave() }
                guard let url else { return }
                // The provider's file disappears when this block returns; keep a copy.
                let name = provider.suggestedName.map { $0 + "." + url.pathExtension } ?? url.lastPathComponent
                let target = dir.appendingPathComponent(name)
                try? FileManager.default.copyItem(at: url, to: target)
                if let d = CoreBridge.shared.storage.describe(target) {
                    lock.lock(); sources.append(d); lock.unlock()
                }
            }
        }
        group.notify(queue: .main) { [weak self] in self?.resolve(id, ["sources": sources]) }
    }

    func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) {
        let id = pickId
        let st = CoreBridge.shared.storage
        switch pickKind {
        case "dir":
            guard let u = urls.first, let mark = st.saveBookmark(for: u) else { resolve(id, nil); return }
            resolve(id, ["uri": mark, "label": u.lastPathComponent])
        case "folder":
            guard let u = urls.first else { resolve(id, nil); return }
            st.keepAccess(u)
            DispatchQueue.global().async { [weak self] in
                let sources = st.walk(folder: u)
                DispatchQueue.main.async { self?.resolve(id, ["sources": sources]) }
            }
        default:
            var sources: [[String: Any]] = []
            for u in urls {
                st.keepAccess(u)
                if let d = st.describe(u) { sources.append(d) }
            }
            resolve(id, ["sources": sources])
        }
    }

    func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) {
        resolve(pickId, nil)
    }

    func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction, decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
        if let u = action.request.url, u.scheme != "sweaxdrop" {
            UIApplication.shared.open(u)
            decisionHandler(.cancel)
            return
        }
        decisionHandler(.allow)
    }
}

/// WKUserContentController keeps its handlers alive; this breaks the cycle.
final class WeakHandler: NSObject, WKScriptMessageHandler {
    weak var target: WKScriptMessageHandler?
    init(_ t: WKScriptMessageHandler) { target = t }
    func userContentController(_ uc: WKUserContentController, didReceive m: WKScriptMessage) {
        target?.userContentController(uc, didReceive: m)
    }
}

/// Serves the bundled UI files for `sweaxdrop://app/...`.
final class AssetHandler: NSObject, WKURLSchemeHandler {
    func webView(_ webView: WKWebView, start task: WKURLSchemeTask) {
        guard let url = task.request.url else { return }
        var path = url.path
        if path.isEmpty || path == "/" { path = "/index.html" }
        let base = Bundle.main.resourceURL!.appendingPathComponent("ui")
        let file = base.appendingPathComponent(String(path.dropFirst()))
        guard file.path.hasPrefix(base.path), let data = try? Data(contentsOf: file) else {
            task.didFailWithError(URLError(.fileDoesNotExist))
            return
        }
        let mime = UTType(filenameExtension: file.pathExtension)?.preferredMIMEType ?? "application/octet-stream"
        let resp = HTTPURLResponse(url: url, statusCode: 200, httpVersion: "HTTP/1.1", headerFields: ["Content-Type": mime, "Content-Length": "\(data.count)"])!
        task.didReceive(resp)
        task.didReceive(data)
        task.didFinish()
    }

    func webView(_ webView: WKWebView, stop task: WKURLSchemeTask) {}
}
