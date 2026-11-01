import UIKit
import UserNotifications

@main
final class AppDelegate: UIResponder, UIApplicationDelegate {
    var window: UIWindow?
    private var controller: WebViewController?

    func application(_ application: UIApplication, didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?) -> Bool {
        UIDeviceName.value = UIDevice.current.name
        CoreBridge.shared.start()
        Discovery.shared.start()
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound]) { _, _ in }

        let w = UIWindow(frame: UIScreen.main.bounds)
        let c = WebViewController()
        w.rootViewController = c
        w.backgroundColor = UIColor(red: 0.047, green: 0.055, blue: 0.071, alpha: 1)
        w.makeKeyAndVisible()
        window = w
        controller = c
        if let u = launchOptions?[.url] as? URL { c.stageIncoming([u]) }
        return true
    }

    /// "Open in SweaxDrop" from Files, Mail, Safari downloads…
    func application(_ app: UIApplication, open url: URL, options: [UIApplication.OpenURLOptionsKey: Any] = [:]) -> Bool {
        controller?.stageIncoming([url])
        return true
    }
}
