import Foundation

/// Bonjour for iOS: announces this phone as `_sweaxdrop._tcp` and resolves
/// the others, passing their addresses to the core (`peer_hint`). The PC and
/// Android apps announce the same service with mDNS.
final class Discovery: NSObject, NetServiceBrowserDelegate, NetServiceDelegate {
    static let shared = Discovery()

    private var published: NetService?
    private let browser = NetServiceBrowser()
    private var resolving: [NetService] = []
    private var hintId: Int32 = 1_000_000

    func start() {
        let core = CoreBridge.shared
        guard core.started, published == nil else { return }
        let svc = NetService(domain: "local.", type: "_sweaxdrop._tcp.", name: core.deviceId, port: Int32(core.port))
        let txt: [String: Data] = [
            "id": Data(core.deviceId.utf8),
            "name": Data(UIDeviceName.value.utf8),
            "plat": Data("ios".utf8),
            "v": Data("1.0.0".utf8),
        ]
        svc.setTXTRecord(NetService.data(fromTXTRecord: txt))
        svc.publish()
        published = svc
        browser.delegate = self
        browser.searchForServices(ofType: "_sweaxdrop._tcp.", inDomain: "local.")
    }

    func netServiceBrowser(_ browser: NetServiceBrowser, didFind service: NetService, moreComing: Bool) {
        guard service.name != CoreBridge.shared.deviceId else { return }
        service.delegate = self
        resolving.append(service)
        service.resolve(withTimeout: 5)
    }

    func netServiceDidResolveAddress(_ sender: NetService) {
        resolving.removeAll { $0 == sender }
        let txt = sender.txtRecordData().map { NetService.dictionary(fromTXTRecord: $0) } ?? [:]
        let str = { (k: String) in txt[k].flatMap { String(data: $0, encoding: .utf8) } ?? "" }
        let id = str("id").isEmpty ? sender.name : str("id")
        var addrs: [String] = []
        for data in sender.addresses ?? [] {
            if let a = Discovery.ipv4(data, port: sender.port) { addrs.append(a) }
        }
        guard !addrs.isEmpty else { return }
        let req: [String: Any] = ["cmd": "peer_hint", "id": id, "name": str("name"), "platform": str("plat"), "addrs": addrs]
        if let d = try? JSONSerialization.data(withJSONObject: req), let s = String(data: d, encoding: .utf8) {
            hintId += 1
            CoreBridge.shared.call(id: hintId, request: s)
        }
    }

    func netService(_ sender: NetService, didNotResolve errorDict: [String: NSNumber]) {
        resolving.removeAll { $0 == sender }
    }

    private static func ipv4(_ data: Data, port: Int) -> String? {
        data.withUnsafeBytes { raw -> String? in
            guard let sa = raw.baseAddress?.assumingMemoryBound(to: sockaddr.self), sa.pointee.sa_family == sa_family_t(AF_INET) else { return nil }
            let sin = raw.baseAddress!.assumingMemoryBound(to: sockaddr_in.self).pointee
            var addr = sin.sin_addr
            var buf = [CChar](repeating: 0, count: Int(INET_ADDRSTRLEN))
            inet_ntop(AF_INET, &addr, &buf, socklen_t(INET_ADDRSTRLEN))
            return "\(String(cString: buf)):\(port)"
        }
    }
}

enum UIDeviceName {
    static var value: String = "iPhone"
}
