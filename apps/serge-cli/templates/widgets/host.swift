// Built into the containing application. The widget extension has its own entry point.
import Foundation
#if os(iOS)
import ActivityKit
#endif

private func widgetError(_ message: String) -> NSError {
    NSError(domain: "org.hyperish.serge.widget", code: 1,
            userInfo: [NSLocalizedDescriptionKey: message])
}

#if os(iOS)
@available(iOS 16.2, *)
private func runActivity<A: ActivityAttributes>(_ attributes: A, operation: Int32, data: Data, id: String) async throws -> String {
    guard data.count <= 4096 else { throw widgetError("Live Activity state exceeds 4 KiB") }
    let state = try JSONDecoder().decode(A.ContentState.self, from: data)
    let content = ActivityContent(state: state, staleDate: nil)
    if operation == 1 {
        return try Activity<A>.request(attributes: attributes, content: content, pushType: nil).id
    }
    guard let activity = Activity<A>.activities.first(where: { $0.id == id }) else {
        throw widgetError("Unknown Live Activity")
    }
    switch operation {
    case 2: await activity.update(content)
    case 3: await activity.end(content, dismissalPolicy: .default)
    default: throw widgetError("Invalid Live Activity operation")
    }
    return activity.id
}
#endif

public typealias SergeWidgetCompletion = @convention(c) (UnsafeMutableRawPointer?, Int32, UnsafePointer<CChar>?) -> Void

@_cdecl("serge_widget_publish_v1")
public func sergeWidgetPublish(_ kind: UnsafePointer<CChar>, _ operation: Int32,
                               _ bytes: UnsafePointer<UInt8>, _ length: Int, _ activityID: UnsafePointer<CChar>,
                               _ context: UnsafeMutableRawPointer?, _ complete: @escaping SergeWidgetCompletion) {
    guard length >= 0 && length <= 65536 else {
        "Invalid widget state size".withCString { complete(context, 1, $0) }
        return
    }
    let data = Data(bytes: bytes, count: length)
    let kind = String(cString: kind), id = String(cString: activityID)
    Task {
        do {
            guard #available(iOS 16.2, macOS 14, *) else { throw widgetError("This OS does not support the widget") }
            let value: String
            switch (kind, operation) {
{{home}}            #if os(iOS)
{{live}}            #endif
            default: throw widgetError("Unknown widget or operation")
            }
            value.withCString { complete(context, 0, $0) }
        } catch {
            error.localizedDescription.withCString { complete(context, 1, $0) }
        }
    }
}
