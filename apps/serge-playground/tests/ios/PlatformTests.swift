import XCTest
import Vision

final class PlatformTests: XCTestCase {
    let app = XCUIApplication(bundleIdentifier: "org.example.ios-sdk")

    override func setUpWithError() throws {
        continueAfterFailure = false
    }

    func start(_ name: String) {
        app.launchEnvironment["SERGE_IOS_PROBE"] = name
        app.launchEnvironment["HYPERISH_RENDER_BACKEND"] = "cpu"
        app.launchArguments = ["-AppleLanguages", "(en)", "-AppleLocale", "en_US"]
        app.launch()
    }

    func tap(_ item: XCUIElement, file: StaticString = #filePath, line: UInt = #line) {
        XCTAssertTrue(item.waitForExistence(timeout: 12), "\(item)\n\(app.debugDescription)", file: file, line: line)
        item.tap()
    }

    func completed(_ mode: String) {
        XCTAssertTrue(app.staticTexts["Completed " + mode].waitForExistence(timeout: 10), app.debugDescription)
    }

    func testAction() {
        start("action")
        tap(app.buttons["Archive project"].firstMatch)
        completed("action")
    }

    func testBiometric() {
        start("biometric")
        let board = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        for host in [board, app] {
            let allow = host.alerts.buttons.matching(NSPredicate(format: "label IN %@", ["OK","Allow"])).firstMatch
            if allow.waitForExistence(timeout: 2) { allow.tap() }
        }
        // The driver supplies a matching simulated face and verifies the LocalAuthentication result.
        completed("biometric")
    }

    func testDateAndTime() {
        start("date")
        tap(app.buttons["Done"].firstMatch)
        completed("date")
        start("time")
        tap(app.buttons["Cancel"].firstMatch)
        completed("time")
    }

    func testShare() {
        start("share")
        tap(app.descendants(matching: .any).matching(NSPredicate(format: "label == %@", "Copy")).firstMatch)
        start("read-shared")
        completed("read-shared")
    }

    func testDocumentRoundTrip() {
        start("export")
        tap(app.buttons["Save"].firstMatch)
        completed("export")
        start("import")
        let items = app.descendants(matching: .any)
        let file = items.matching(NSPredicate(format: "label BEGINSWITH[c] %@", "serge-export")).firstMatch
        if !file.waitForExistence(timeout: 2) {
            let browse = app.buttons["Browse"].firstMatch
            if browse.exists { browse.tap() }
            let local = items.matching(NSPredicate(format: "label == %@", "On My iPhone")).firstMatch
            if local.waitForExistence(timeout: 4) { local.tap() }
        }
        tap(file)
        let open = app.buttons["Open"].firstMatch
        if open.exists { open.tap() }
        completed("import")
    }

    func testCancelImport() {
        start("import-cancel")
        let navigation = app.navigationBars["FullDocumentManagerViewControllerNavigationBar"]
        tap(navigation.buttons["Cancel"].firstMatch)
        completed("import-cancel")
    }

    func testInjectedNotificationAction() {
        start("push")
        let board = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        let permission = board.alerts.buttons["Allow"].firstMatch
        if permission.waitForExistence(timeout: 5) { permission.tap() }
        let title = board.descendants(matching: .any).matching(NSPredicate(
            format: "label CONTAINS[c] %@", "Serge export ready")).firstMatch
        XCTAssertTrue(title.waitForExistence(timeout: 20), board.debugDescription)
        title.press(forDuration: 1.5)
        let command = board.descendants(matching: .any).matching(NSPredicate(
            format: "label == %@", "Archive project")).firstMatch
        tap(command)
    }

    func testWidgetOnHomeScreen() throws {
        app.activate()
        XCUIDevice.shared.press(.home)
        let board = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        let icon = board.icons["ios-sdk"].firstMatch
        for _ in 0..<3 where !icon.exists { board.swipeLeft() }
        XCTAssertTrue(icon.waitForExistence(timeout: 5), board.debugDescription)
        icon.press(forDuration: 1.2)
        let items = board.descendants(matching: .any)
        func labeled(_ name: String) -> XCUIElement {
            items.matching(NSPredicate(format: "label ==[c] %@", name)).firstMatch
        }
        tap(labeled("Edit Home Screen"))
        if !labeled("Add Widget").exists { tap(labeled("Edit")) }
        tap(labeled("Add Widget"))
        let search = board.searchFields["Search Widgets"].firstMatch
        tap(search)
        search.typeText("ios-sdk")
        let matches = items.matching(NSPredicate(format: "label ==[c] %@ OR label == %@", "ios-sdk", "ExportHome"))
        let galleryItem = matches.allElementsBoundByIndex.first { $0.elementType != .icon && $0.isHittable }
        XCTAssertNotNil(galleryItem, board.debugDescription)
        if let item = galleryItem { tap(item) }
        let add = board.buttons.matching(NSPredicate(format: "label ENDSWITH[c] %@", "Add Widget")).firstMatch
        XCTAssertTrue(add.waitForExistence(timeout: 12), board.debugDescription)
        add.tap()
        let done = labeled("Done")
        if done.waitForExistence(timeout: 3) { done.tap() }
        let saved = items.matching(NSPredicate(format: "label CONTAINS[c] %@", "Saved project")).firstMatch
        let until = Date().addingTimeInterval(30)
        var words = ""
        repeat {
            if saved.exists { return }
            // SpringBoard can expose a widget as a single icon. Check its rendered text as well.
            let bitmap = try XCTUnwrap(board.screenshot().image.cgImage)
            let request = VNRecognizeTextRequest()
            request.recognitionLanguages = ["en-US"]
            try VNImageRequestHandler(cgImage: bitmap).perform([request])
            words = (request.results ?? []).compactMap { $0.topCandidates(1).first?.string }.joined(separator: " ")
            if words.localizedCaseInsensitiveContains("Saved project") { return }
            Thread.sleep(forTimeInterval: 1)
        } while Date() < until
        XCTFail("Rendered Home screen: \(words)\n\(board.debugDescription)")
    }
}
