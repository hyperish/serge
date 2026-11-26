import XCTest

final class LockWidget: XCTestCase {
    func testPlaceExportOnLockScreen() throws {
        continueAfterFailure = false
        let board = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        let prefs = XCUIApplication(bundleIdentifier: "com.apple.Preferences")
        let poster = XCUIApplication(bundleIdentifier: "com.apple.PosterBoard")

        func matching(_ app: XCUIApplication, _ rule: String, _ value: String) -> XCUIElementQuery {
            app.descendants(matching: .any).matching(NSPredicate(format: rule, value))
        }
        func visible(_ apps: [XCUIApplication], _ rule: String, _ value: String, timeout: TimeInterval = 8) -> XCUIElement? {
            let deadline = Date().addingTimeInterval(timeout)
            repeat {
                for app in apps where app.state == .runningForeground || app == board {
                    if let element = matching(app, rule, value).allElementsBoundByIndex.first(where: { $0.isHittable }) { return element }
                }
                Thread.sleep(forTimeInterval: 0.25)
            } while Date() < deadline
            return nil
        }
        func dumpUI(_ stage: String) -> String {
            let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
            attachment.name = stage
            attachment.lifetime = .keepAlways
            add(attachment)
            return [prefs,board,poster].filter { $0.state == .runningForeground || $0 == board }.map { $0.debugDescription }.joined(separator: "\n")
        }
        prefs.launch()
        let wallpaper = matching(prefs, "label ==[c] %@", "Wallpaper").firstMatch
        for _ in 0..<6 {
            if wallpaper.exists && wallpaper.isHittable { break }
            prefs.swipeUp()
        }
        let surfaces = [prefs, board, poster]
        if wallpaper.exists && wallpaper.isHittable {
            wallpaper.tap()
            let customize = try XCTUnwrap(visible(surfaces, "label CONTAINS[c] %@", "Customize"), dumpUI("Wallpaper"))
            customize.tap()
        } else {
            XCUIDevice.shared.press(.home)
            let top = board.coordinate(withNormalizedOffset: CGVector(dx: 0.20, dy: 0.005))
            let bottom = board.coordinate(withNormalizedOffset: CGVector(dx: 0.20, dy: 0.92))
            top.press(forDuration: 0.15, thenDragTo: bottom)
            Thread.sleep(forTimeInterval: 1)
            print("Notification Center", board.debugDescription)
            board.coordinate(withNormalizedOffset: CGVector(dx: 0.34, dy: 0.50)).press(forDuration: 1.8)
            let customize = try XCTUnwrap(visible([board,poster], "label CONTAINS[c] %@", "Customi"), dumpUI("Wallpaper collection"))
            customize.tap()
            let lock = try XCTUnwrap(visible([poster, board], "label CONTAINS[c] %@", "Lock Screen Wallpaper"), dumpUI("Customize Wallpaper"))
            lock.tap()
        }
        let well = poster.buttons["grouped-widgets-reticle-view"].firstMatch
        XCTAssertTrue(well.waitForExistence(timeout: 12), dumpUI("Lock Screen editor"))
        let point = CGVector(dx: well.frame.midX, dy: well.frame.midY)
        poster.coordinate(withNormalizedOffset: .zero).withOffset(point).tap()
        Thread.sleep(forTimeInterval: 1)
        print("Opened widget chooser", poster.debugDescription)
        var widgetApp = visible(surfaces, "label ==[c] %@", "ios-sdk", timeout: 2)
        for _ in 0..<5 where widgetApp == nil {
            let top = poster.state == .runningForeground ? poster : board
            let from = top.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.91))
            from.press(forDuration: 0.1, thenDragTo: top.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.53)))
            widgetApp = visible(surfaces, "label ==[c] %@", "ios-sdk", timeout: 1)
        }
        try XCTUnwrap(widgetApp, dumpUI("Lock Screen widget chooser")).tap()
        let panel = poster.state == .runningForeground ? poster : board
        print("Accessory gallery", panel.debugDescription)
        let previews = panel.buttons.matching(NSPredicate(format: "label CONTAINS[c] %@ OR value CONTAINS[c] %@", "ExportHome", "Widget"))
        if let preview = previews.allElementsBoundByIndex.first(where: { $0.isHittable }) {
            preview.tap()
        } else {
            let preview = try XCTUnwrap(visible(surfaces, "label CONTAINS[c] %@", "ExportHome"), dumpUI("Accessory gallery"))
            preview.tap()
        }
        // The detailed preview and widget chooser can dismiss in the same animation.
        for _ in 0..<2 {
            Thread.sleep(forTimeInterval: 0.9)
            let button = panel.buttons.matching(NSPredicate(format: "label ==[c] %@", "close")).firstMatch
            guard button.exists else { break }
            let frame = button.frame
            panel.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: frame.midX, dy: frame.midY)).tap()
        }
        let done = poster.buttons["editing-done"].firstMatch
        XCTAssertTrue(done.waitForExistence(timeout: 6), dumpUI("After adding the accessory"))
        let frame = done.frame
        poster.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: frame.midX, dy: frame.midY)).tap()
        let apply = board.buttons["Set as Wallpaper Pair"].firstMatch
        if apply.waitForExistence(timeout: 2) { apply.tap() }
        XCUIDevice.shared.press(.home)
        print("Lock screen widget placement saved")
    }
}
