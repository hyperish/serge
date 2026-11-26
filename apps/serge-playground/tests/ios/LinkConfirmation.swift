import UIKit
import XCTest

final class LinkConfirmation: XCTestCase {
    func testOpenRegisteredProjectLinks() throws {
        let directory = FileManager.default.temporaryDirectory
        try Data().write(to: directory.appendingPathComponent("links-ready"), options: .atomic)
        let finished = directory.appendingPathComponent("links-finished")
        let board = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        let until = Date().addingTimeInterval(170)
        while Date() < until {
            if FileManager.default.fileExists(atPath: finished.path) { return }
            let alert = board.alerts.firstMatch
            if alert.exists {
                let names = [alert.label] + alert.staticTexts.allElementsBoundByIndex.map(\.label)
                let open = alert.buttons["Open"]
                if names.contains(where: { $0.contains("ios-slot") }) && open.exists {
                    open.tap()
                }
            }
            Thread.sleep(forTimeInterval: 0.2)
        }
        XCTFail("The installed app did not finish the two URL scenarios")
    }
}
