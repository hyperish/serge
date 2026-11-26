import AppKit
import ApplicationServices

// The app supplies the live AppKit button center in screen points.
guard CommandLine.arguments.count == 4,
      let x = Double(CommandLine.arguments[1]), let y = Double(CommandLine.arguments[2]),
      let pid = Int32(CommandLine.arguments[3]),
      CGPreflightPostEventAccess() else {
    fatalError("The test session must allow posting pointer events to the unlocked menu bar")
}
func attribute(_ node: AXUIElement, _ name: String) -> CFTypeRef? {
    var value: CFTypeRef?
    return AXUIElementCopyAttributeValue(node, name as CFString, &value) == .success ? value : nil
}

let screen = CGDisplayBounds(CGMainDisplayID())
func visibleCenter(_ node: AXUIElement, depth: Int) -> CGPoint? {
    let labels = [kAXTitleAttribute, kAXDescriptionAttribute, kAXValueAttribute]
        .compactMap { attribute(node, $0 as String) as? String }
    if labels.contains("Projects"),
       let position = attribute(node, kAXPositionAttribute as String),
       let extent = attribute(node, kAXSizeAttribute as String) {
        var point = CGPoint.zero, size = CGSize.zero
        if AXValueGetValue(position as! AXValue, .cgPoint, &point),
           AXValueGetValue(extent as! AXValue, .cgSize, &size) {
            let center = CGPoint(x: point.x + size.width/2, y: point.y + size.height/2)
            if size.width > 0 && size.height > 0 && screen.contains(center) { return center }
        }
    }
    if depth > 0, let children = attribute(node, kAXChildrenAttribute as String) as? [AXUIElement] {
        for child in children {
            if let center = visibleCenter(child, depth: depth-1) { return center }
        }
    }
    return nil
}

let applications = [pid] + NSWorkspace.shared.runningApplications
    .filter { ["com.apple.controlcenter", "com.apple.systemuiserver"].contains($0.bundleIdentifier ?? "") }
    .map(\.processIdentifier)
let fallback = CGPoint(x: x, y: screen.height - y)
var target: CGPoint?
let until = Date().addingTimeInterval(7)
repeat {
    for id in applications {
        let app = AXUIElementCreateApplication(id)
        AXUIElementSetMessagingTimeout(app, 0.5)
        for key in ["AXExtrasMenuBar", kAXMenuBarAttribute as String] {
            if let menu = attribute(app, key), let center = visibleCenter(menu as! AXUIElement, depth: 2) {
                target = center
                break
            }
        }
        if target != nil { break }
    }
    if target == nil && screen.contains(fallback) { target = fallback }
    if target == nil { Thread.sleep(forTimeInterval: 0.2) }
} while target == nil && Date() < until
guard let point = target else {
    print("No visible Projects status item. Screen \(screen), AppKit center \(fallback), accessibility \(AXIsProcessTrusted())")
    exit(1)
}
print("Clicking visible Projects status item at \(point)")
let source = CGEventSource(stateID: .hidSystemState)
for type in [CGEventType.mouseMoved, .leftMouseDown, .leftMouseUp] {
    guard let event = CGEvent(mouseEventSource: source, mouseType: type,
                              mouseCursorPosition: point, mouseButton: .left) else {
        fatalError("Could not create the status-bar pointer event")
    }
    if type != .mouseMoved {
        event.setIntegerValueField(.mouseEventClickState, value: 1)
    }
    event.post(tap: .cghidEventTap)
    Thread.sleep(forTimeInterval: 0.04)
}
