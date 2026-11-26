import UIKit

@main final class Host: UIResponder, UIApplicationDelegate {
    var window: UIWindow?

    func application(_ application: UIApplication, didFinishLaunchingWithOptions options: [UIApplication.LaunchOptionsKey: Any]?) -> Bool {
        let open = UIButton(type: .system)
        open.setTitle("Open project", for: .normal)
        open.addTarget(self, action: #selector(openProject), for: .touchUpInside)
        let controller = UIViewController()
        controller.view.backgroundColor = .systemBackground
        open.translatesAutoresizingMaskIntoConstraints = false
        controller.view.addSubview(open)
        NSLayoutConstraint.activate([
            open.centerXAnchor.constraint(equalTo: controller.view.centerXAnchor),
            open.centerYAnchor.constraint(equalTo: controller.view.centerYAnchor),
        ])
        window = UIWindow(frame: UIScreen.main.bounds)
        window?.rootViewController = controller
        window?.makeKeyAndVisible()
        return true
    }

    @objc private func openProject() {
        guard let value = ProcessInfo.processInfo.environment["SERGE_PROJECT_LINK"],
              let url = URL(string: value) else { return }
        UIApplication.shared.open(url)
    }
}
