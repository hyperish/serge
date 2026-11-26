import UIKit
import XCTest

final class LinkConfirmation: XCTestCase {
    func testOpenRegisteredProjectLinks() throws {
        let directory = FileManager.default.temporaryDirectory
        try Data().write(to: directory.appendingPathComponent("links-ready"), options: .atomic)
        let finished = directory.appendingPathComponent("links-finished")
        let board = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        let sender = XCUIApplication(bundleIdentifier: "org.hyperish.verification.host")
        var requested = Set<String>()
        let until = Date().addingTimeInterval(240)
        while Date() < until {
            if FileManager.default.fileExists(atPath: finished.path) {
                XCTAssertEqual(requested, Set(["JSON", "archive"]))
                return
            }
            for rail in ["JSON", "archive"] where !requested.contains(rail) {
                let request = directory.appendingPathComponent("open-" + rail)
                guard let url = try? String(contentsOf: request, encoding: .utf8) else { continue }
                sender.launchEnvironment["SERGE_PROJECT_LINK"] = url
                sender.launch()
                let button = sender.buttons["Open project"]
                XCTAssertTrue(button.waitForExistence(timeout: 15), sender.debugDescription)
                button.tap()
                requested.insert(rail)
            }
            let hosts = sender.state == .runningForeground ? [board, sender] : [board]
            for host in hosts {
                let alert = host.alerts.firstMatch
                guard alert.exists else { continue }
                let names = [alert.label] + alert.staticTexts.allElementsBoundByIndex.map(\.label)
                let open = alert.buttons["Open"]
                if names.contains(where: { $0.contains("ios-slot") }) && open.exists && open.isHittable {
                    open.tap()
                }
            }
            Thread.sleep(forTimeInterval: 0.2)
        }
        XCTFail("The installed app did not finish the two URL scenarios")
    }
}
