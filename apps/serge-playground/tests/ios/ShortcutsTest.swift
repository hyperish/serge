import XCTest

final class ShortcutsTest: XCTestCase {
    func testRunExportHomeFromTheEditor() throws {
        continueAfterFailure = false
        let shortcuts = XCUIApplication(bundleIdentifier: "com.apple.shortcuts")
        let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        for host in [springboard, shortcuts] {
            let open = host.alerts.buttons["Open"].firstMatch
            if open.waitForExistence(timeout: 1) { open.tap() }
        }
        shortcuts.activate()
        let welcome = shortcuts.buttons["Continue"].firstMatch
        if welcome.waitForExistence(timeout: 2) { welcome.tap() }
        func find(_ name: String) -> XCUIElement {
            shortcuts.descendants(matching: .any).matching(NSPredicate(format: "label ==[c] %@", name)).firstMatch
        }
        let fields = shortcuts.descendants(matching: .any).matching(NSPredicate(
            format: "placeholderValue ==[c] %@ OR label ==[c] %@", "Search Actions", "Search Actions"))
        func add(_ title: String) {
            let search = fields.firstMatch
            XCTAssertTrue(search.waitForExistence(timeout: 12), shortcuts.debugDescription)
            search.tap()
            // The compact editor field can be replaced by an expanded results field.
            let editing = fields.firstMatch
            editing.typeText(title)
            let result = find(title)
            if title == "Read ExportHome" {
                let until = Date().addingTimeInterval(90)
                while !result.waitForExistence(timeout: 6) && Date() < until {
                    // A query issued before indexing finishes can keep its empty result.
                    let clear = shortcuts.buttons["Clear search field"].firstMatch
                    guard clear.exists else { break }
                    clear.tap()
                    let provider = shortcuts.tables.cells.containing(.staticText, identifier: "ios-sdk").firstMatch
                    if provider.waitForExistence(timeout: 3) {
                        provider.tap()
                        break
                    }
                    fields.firstMatch.typeText(title)
                }
            }
            XCTAssertTrue(result.waitForExistence(timeout: 20), shortcuts.debugDescription)
            result.tap()
        }
        add("Read ExportHome")
        add("Copy to Clipboard")
        let run = shortcuts.buttons.matching(NSPredicate(format: "label IN %@", ["Run Shortcut", "Run", "Play"])).firstMatch
        XCTAssertTrue(run.waitForExistence(timeout: 6), shortcuts.debugDescription)
        run.tap()
        for host in [shortcuts, springboard] {
            let allow = host.alerts.buttons.matching(NSPredicate(format: "label IN %@", ["Allow", "Always Allow", "Allow Once"])).firstMatch
            if allow.waitForExistence(timeout: 1) { allow.tap() }
        }
    }
}
