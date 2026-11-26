import AppIntents
import Foundation
import WidgetKit

// Compiled in the same module as the generated intent for the simulator fixture.
@_cdecl("serge_test_read_export_intent")
public func readExportIntent() {
    Task { @MainActor in
        let receipt = FileManager.default.temporaryDirectory.appendingPathComponent("serge-intent.txt")
        do {
            guard #available(iOS 16.2, *) else { throw CocoaError(.featureUnsupported) }
            let result = try await ReadExportHomeIntent().perform()
            guard let value = result.value else { throw CocoaError(.fileReadCorruptFile) }
            try Data(value.utf8).write(to: receipt, options: .atomic)
        } catch {
            try? Data("error: \(error)".utf8).write(to: receipt, options: .atomic)
        }
    }
}

#if os(iOS)
@_cdecl("serge_test_widget_families")
public func readWidgetFamilies() {
    let receipt = FileManager.default.temporaryDirectory.appendingPathComponent("serge-widget-families.txt")
    WidgetCenter.shared.getCurrentConfigurations { result in
        do {
            let families = try result.get().filter { $0.kind == "ExportHome" }.map { info in
                if #available(iOS 16.0, *), info.family == .accessoryRectangular {
                    return "accessoryRectangular"
                }
                return info.family == .systemSmall ? "systemSmall" : String(describing: info.family)
            }
            try JSONEncoder().encode(families).write(to: receipt, options: .atomic)
        } catch {
            try? Data("error: \(error)".utf8).write(to: receipt, options: .atomic)
        }
    }
}
#endif
