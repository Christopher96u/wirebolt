import AppKit

// SwiftUI AX nodes expose the Objective-C selector without protocol conformance.
// AnyObject's dynamic dispatch preserves its BOOL signature without an IMP cast.
@objc private protocol LocalAccessibilityAction {
    func accessibilityPerformPress() -> Bool
}

struct FixtureRunner: RequestRunner {
    func events(for input: RunInput, runID: RunID) -> AsyncThrowingStream<RunEvent, any Error> {
        AsyncThrowingStream { $0.finish() }
    }
    func cancel(runID: RunID) {}
}

@MainActor enum NativeTestSupport {
    struct Element {
        let object: NSObject
        let role: String
        let name: String
        let frame: NSRect
    }
    static func elements(_ object: Any, depth: Int = 0) -> [Element] {
        guard depth < 40, let object = object as? NSObject else { return [] }
        func value(_ key: String) -> Any? {
            object.responds(to: NSSelectorFromString(key)) ? object.value(forKey: key) : nil
        }
        func text(_ key: String) -> String {
            let raw = value(key)
            return (raw as? NSAttributedString)?.string ?? (raw as? String) ?? ""
        }
        let name = ["accessibilityLabel", "accessibilityTitle", "accessibilityValue"].map(text).first { !$0.isEmpty } ?? ""
        return [Element(object: object, role: text("accessibilityRole"), name: name,
            frame: (value("accessibilityFrame") as? NSValue)?.rectValue ?? .zero)]
            + (value("accessibilityChildren") as? [Any] ?? []).flatMap { elements($0, depth: depth + 1) }
    }
    @discardableResult
    static func press(_ object: Any, named name: String) -> Bool {
        guard let item = elements(object).first(where: { ["AXButton", "AXMenuItem"].contains($0.role) && $0.name == name }) else { return false }
        if let button = item.object as? NSButton { button.performClick(nil); return true }
        guard item.object.responds(to: NSSelectorFromString("accessibilityPerformPress")) else { return false }
        _ = (item.object as AnyObject).accessibilityPerformPress?()
        return true
    }
    static func flush(_ host: NSView) {
        host.layoutSubtreeIfNeeded()
        host.displayIfNeeded()
    }
    static func settle(_ host: NSView) async {
        for _ in 0..<10 {
            try? await Task.sleep(for: .milliseconds(12))
            flush(host)
        }
    }
    static func wait(_ host: NSView, timeout: Duration = .seconds(5), until condition: () -> Bool) async {
        let deadline = ContinuousClock.now.advanced(by: timeout)
        while !condition() && ContinuousClock.now < deadline {
            try? await Task.sleep(for: .milliseconds(10))
            flush(host)
        }
        precondition(condition(), "Native state did not become ready before the deadline")
    }
    static func capture(_ host: NSView, directory: URL, name: String) throws {
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        flush(host)
        guard let bitmap = host.bitmapImageRepForCachingDisplay(in: host.bounds) else { preconditionFailure("In-process capture failed") }
        host.cacheDisplay(in: host.bounds, to: bitmap)
        try bitmap.representation(using: .png, properties: [:])!.write(to: directory.appending(path: name + ".png"))
    }
    // These process-local AX flags have no NSAccessibilityProtocol equivalents.
    // Preserve them through the legacy attribute API used to enable SwiftUI's tree.
    static func enableAccessibility() -> () -> Void {
        let attributes = ["AXEnhancedUserInterface", "AXManualAccessibility"].map { NSAccessibility.Attribute(rawValue: $0) }
        let previous = attributes.map { NSApp.accessibilityAttributeValue($0) }
        for attribute in attributes { NSApp.accessibilitySetValue(true, forAttribute: attribute) }
        return {
            for (attribute, value) in zip(attributes, previous) { NSApp.accessibilitySetValue(value ?? false, forAttribute: attribute) }
        }
    }
}
