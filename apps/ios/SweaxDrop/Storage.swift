import Foundation
import Photos
import UniformTypeIdentifiers

/// Received files: photos and videos go to the photo library (if allowed),
/// everything else to Documents/SweaxDrop, which the Files app shows under
/// "On My iPhone › SweaxDrop". A folder picked in Settings overrides that.
final class Storage {
    private let fm = FileManager.default
    private let lock = NSLock()
    /// Security-scoped folders we keep open while the app runs.
    private var scoped: [URL] = []

    var documents: URL {
        fm.urls(for: .documentDirectory, in: .userDomainMask)[0].appendingPathComponent("SweaxDrop", isDirectory: true)
    }

    private var staging: URL {
        fm.temporaryDirectory.appendingPathComponent("incoming", isDirectory: true)
    }

    func keepAccess(_ url: URL) {
        if url.startAccessingSecurityScopedResource() {
            lock.lock(); scoped.append(url); lock.unlock()
        }
    }

    // MARK: sending

    func openRead(_ token: String) -> Int32 {
        let path = token.hasPrefix("file://") ? (URL(string: token)?.path ?? token) : token
        return open(path, O_RDONLY)
    }

    // MARK: receiving

    func createOutput(_ reqJson: String) -> String? {
        guard let data = reqJson.data(using: .utf8),
              let req = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return nil }
        let rel = (req["path"] as? String ?? "file").split(separator: "/").map(String.init).filter { $0 != ".." && !$0.isEmpty }
        let mime = req["mime"] as? String ?? ""
        let gallery = (req["gallery"] as? Bool ?? true) && (mime.hasPrefix("image/") || mime.hasPrefix("video/"))
        let dest = req["dest"] as? String ?? ""
        let name = rel.last ?? "file"
        var dir: URL
        var kind = "doc"
        if gallery {
            dir = staging.appendingPathComponent(UUID().uuidString, isDirectory: true)
            kind = "photo"
        } else if !dest.isEmpty, let base = resolveBookmark(dest) {
            dir = base
            for part in rel.dropLast() { dir.appendPathComponent(part, isDirectory: true) }
        } else {
            dir = documents
            for part in rel.dropLast() { dir.appendPathComponent(part, isDirectory: true) }
        }
        do {
            try fm.createDirectory(at: dir, withIntermediateDirectories: true)
        } catch { return nil }
        let url = unique(dir.appendingPathComponent(name))
        let fd = open(url.path, O_RDWR | O_CREAT | O_TRUNC, 0o644)
        guard fd >= 0 else { return nil }
        let handle = "\(kind)|\(url.path)"
        let obj: [String: Any] = ["fd": Int(fd), "handle": handle]
        guard let out = try? JSONSerialization.data(withJSONObject: obj) else { close(fd); return nil }
        return String(data: out, encoding: .utf8)
    }

    func finishOutput(_ handle: String, ok: Bool, mtime: Int64) -> String {
        let parts = handle.split(separator: "|", maxSplits: 1).map(String.init)
        guard parts.count == 2 else { return "" }
        let url = URL(fileURLWithPath: parts[1])
        if !ok {
            try? fm.removeItem(at: url)
            return ""
        }
        if mtime > 0 {
            try? fm.setAttributes([.modificationDate: Date(timeIntervalSince1970: TimeInterval(mtime) / 1000)], ofItemAtPath: url.path)
        }
        if parts[0] == "photo" {
            if saveToPhotos(url) {
                try? fm.removeItem(at: url.deletingLastPathComponent())
                return "Photos"
            }
            // No permission: keep it in Files instead.
            try? fm.createDirectory(at: documents, withIntermediateDirectories: true)
            let target = unique(documents.appendingPathComponent(url.lastPathComponent))
            try? fm.moveItem(at: url, to: target)
            return target.path
        }
        return url.path
    }

    private func saveToPhotos(_ url: URL) -> Bool {
        let sem = DispatchSemaphore(value: 0)
        var status = PHPhotoLibrary.authorizationStatus(for: .addOnly)
        if status == .notDetermined {
            PHPhotoLibrary.requestAuthorization(for: .addOnly) { s in
                status = s
                sem.signal()
            }
            sem.wait()
        }
        guard status == .authorized || status == .limited else { return false }
        var saved = false
        let isVideo = UTType(filenameExtension: url.pathExtension)?.conforms(to: .movie) ?? false
        PHPhotoLibrary.shared().performChanges({
            let req = PHAssetCreationRequest.forAsset()
            let opts = PHAssetResourceCreationOptions()
            opts.originalFilename = url.lastPathComponent
            req.addResource(with: isVideo ? .video : .photo, fileURL: url, options: opts)
        }, completionHandler: { ok, _ in
            saved = ok
            sem.signal()
        })
        sem.wait()
        return saved
    }

    private func unique(_ url: URL) -> URL {
        guard fm.fileExists(atPath: url.path) else { return url }
        let stem = url.deletingPathExtension().lastPathComponent
        let ext = url.pathExtension
        for n in 2... {
            let c = url.deletingLastPathComponent().appendingPathComponent(ext.isEmpty ? "\(stem) (\(n))" : "\(stem) (\(n)).\(ext)")
            if !fm.fileExists(atPath: c.path) { return c }
        }
        return url
    }

    // MARK: custom folder

    func saveBookmark(for url: URL) -> String? {
        guard url.startAccessingSecurityScopedResource() else { return nil }
        defer { url.stopAccessingSecurityScopedResource() }
        guard let data = try? url.bookmarkData(options: [], includingResourceValuesForKeys: nil, relativeTo: nil) else { return nil }
        return "bookmark:" + data.base64EncodedString()
    }

    private func resolveBookmark(_ dest: String) -> URL? {
        guard dest.hasPrefix("bookmark:"), let data = Data(base64Encoded: String(dest.dropFirst(9))) else { return nil }
        var stale = false
        guard let url = try? URL(resolvingBookmarkData: data, options: [], relativeTo: nil, bookmarkDataIsStale: &stale) else { return nil }
        keepAccess(url)
        return url
    }

    // MARK: describing picked files

    func describe(_ url: URL, relative: String? = nil) -> [String: Any]? {
        let values = try? url.resourceValues(forKeys: [.fileSizeKey, .contentModificationDateKey, .contentTypeKey, .isDirectoryKey])
        if values?.isDirectory == true { return nil }
        guard let size = values?.fileSize else { return nil }
        var d: [String: Any] = ["token": url.path, "path": relative ?? url.lastPathComponent, "size": size]
        if let m = values?.contentType?.preferredMIMEType { d["mime"] = m }
        if let t = values?.contentModificationDate { d["mtime"] = Int64(t.timeIntervalSince1970 * 1000) }
        return d
    }

    func walk(folder: URL) -> [[String: Any]] {
        var out: [[String: Any]] = []
        let base = folder.lastPathComponent
        guard let e = fm.enumerator(at: folder, includingPropertiesForKeys: [.isDirectoryKey, .fileSizeKey], options: [.skipsHiddenFiles]) else { return out }
        for case let url as URL in e {
            let rel = url.path.replacingOccurrences(of: folder.path, with: "").trimmingCharacters(in: CharacterSet(charactersIn: "/"))
            if let d = describe(url, relative: "\(base)/\(rel)") { out.append(d) }
        }
        return out
    }
}
