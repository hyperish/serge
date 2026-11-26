import AppIntents
import Foundation
import SwiftUI
import WidgetKit

@available(iOS 16.2, *)
struct {{name}}State: Codable, Equatable {
{{fields}}{{keys}}}

@available(iOS 16.2, *)
enum {{name}}Store {
    private static func fileURL() throws -> URL {
        guard let group = Bundle.main.object(forInfoDictionaryKey: "SergeWidgetAppGroup") as? String,
              let container = FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: group)
        else { throw CocoaError(.fileNoSuchFile) }
        return container.appendingPathComponent("serge-widget-{{name}}.json")
    }

    static func read() throws -> {{name}}State {
        let data = try Data(contentsOf: fileURL())
        return try JSONDecoder().decode({{name}}State.self, from: data)
    }

    static func publish(_ data: Data) throws {
        guard data.count <= 65_536 else { throw CocoaError(.fileWriteOutOfSpace) }
        let state = try JSONDecoder().decode({{name}}State.self, from: data)
        if (try? read()) == state { return }
        try JSONEncoder().encode(state).write(to: fileURL(), options: [.atomic, .completeFileProtection])
        WidgetCenter.shared.reloadTimelines(ofKind: "{{name}}")
    }
}

@available(iOS 16.2, *)
struct {{name}}Entry: TimelineEntry {
    let date: Date
    let state: {{name}}State
}

@available(iOS 16.2, *)
struct {{name}}Provider: TimelineProvider {
    func placeholder(in context: Context) -> {{name}}Entry {
        {{name}}Entry(date: Date(), state: {{name}}State())
    }
    private func current() -> {{name}}Entry {
        {{name}}Entry(date: Date(), state: (try? {{name}}Store.read()) ?? {{name}}State())
    }
    func getSnapshot(in context: Context, completion: @escaping ({{name}}Entry) -> Void) {
        completion(current())
    }
    func getTimeline(in context: Context, completion: @escaping (Timeline<{{name}}Entry>) -> Void) {
        completion(Timeline(entries: [current()], policy: .never))
    }
}

@available(iOS 16.2, *)
struct {{name}}: Widget {
    var body: some WidgetConfiguration {
        StaticConfiguration(kind: "{{name}}", provider: {{name}}Provider()) { entry in
            let content = Group {
{{view}}            }.privacySensitive()
            if #available(iOS 17.0, macOS 14.0, *) {
                content.containerBackground(.background, for: .widget)
            } else {
                content
            }
        }
        .configurationDisplayName("{{name}}")
        #if os(iOS)
        .supportedFamilies([.systemSmall, .systemMedium, .systemLarge, .accessoryRectangular])
        #else
        .supportedFamilies([.systemSmall, .systemMedium, .systemLarge])
        #endif
    }
}

@available(iOS 16.2, *)
struct Read{{name}}Intent: AppIntent {
    static var title: LocalizedStringResource = "Read {{name}}"
    static var authenticationPolicy: IntentAuthenticationPolicy { .requiresAuthentication }
    func perform() async throws -> some IntentResult & ReturnsValue<String> {
        let data = try JSONEncoder().encode({{name}}Store.read())
        return .result(value: String(decoding: data, as: UTF8.self))
    }
}
