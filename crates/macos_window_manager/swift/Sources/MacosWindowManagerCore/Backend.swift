import AppKit
import ApplicationServices
import CoreGraphics
import Foundation

public struct Backend {
    let system: any BackendSystem

    public init() {
        self.system = LiveSystem()
    }

    public init(system: any BackendSystem) {
        self.system = system
    }

    public static func smokeTest() -> Int32 {
        0
    }

    public func validateEnvironment() throws {
        try Environment.validate(system: system)
    }

    public func desktopSnapshot() throws -> DesktopSnapshot {
        try DesktopSnapshotBuilder.build(system: system)
    }

    public func topologySnapshot() throws -> DesktopSnapshot {
        try DesktopSnapshotBuilder.buildTopology(system: system)
    }
}

final class LiveSystem: BackendSystem {
    private let symbols = PrivateSymbolResolver()

    func resolveSymbol<T>(_ symbol: String, as type: T.Type = T.self) -> T? {
        symbols.resolve(symbol, as: type)
    }

    func hasSymbol(_ symbol: String) -> Bool {
        symbols.hasSymbol(symbol)
    }

    func isAccessibilityTrusted() -> Bool {
        typealias Function = @convention(c) () -> DarwinBoolean
        guard let function = symbols.resolve("AXIsProcessTrusted", as: Function.self) else {
            return false
        }

        return function().boolValue
    }

    func mainConnectionID() -> UInt32? {
        typealias Function = @convention(c) () -> UInt32
        guard let function = symbols.resolve("SLSMainConnectionID", as: Function.self) else {
            return nil
        }

        let connectionID = function()
        return connectionID == 0 ? nil : connectionID
    }

    func managedDisplaySpaces() throws -> [[String: Any]] {
        typealias Function = @convention(c) (UInt32) -> Unmanaged<CFArray>?
        guard let function = symbols.resolve("SLSCopyManagedDisplaySpaces", as: Function.self) else {
            throw BackendError.missingTopology("SLSCopyManagedDisplaySpaces")
        }
        guard let connectionID = mainConnectionID() else {
            throw BackendError.missingTopology("SLSMainConnectionID")
        }
        guard let payload = function(connectionID)?.takeRetainedValue() as? [[String: Any]] else {
            throw BackendError.missingTopology("SLSCopyManagedDisplaySpaces")
        }

        return payload
    }

    func windowsForSpace(_ spaceID: UInt64) throws -> [UInt64] {
        typealias Function = @convention(c) (
            UInt32,
            UInt32,
            CFArray,
            Int32,
            UnsafeMutablePointer<Int64>,
            UnsafeMutablePointer<Int64>
        ) -> Unmanaged<CFArray>?
        guard let function = symbols.resolve("SLSCopyWindowsWithOptionsAndTags", as: Function.self) else {
            throw BackendError.missingTopology("SLSCopyWindowsWithOptionsAndTags")
        }
        guard let connectionID = mainConnectionID() else {
            throw BackendError.missingTopology("SLSMainConnectionID")
        }

        var setTags: Int64 = 0
        var clearTags: Int64 = 0
        let spaceList = [NSNumber(value: spaceID)] as CFArray
        guard let payload = function(connectionID, 0, spaceList, 0x2, &setTags, &clearTags)?
            .takeRetainedValue() as? [NSNumber]
        else {
            throw BackendError.missingTopology("SLSCopyWindowsWithOptionsAndTags")
        }

        return payload.map { UInt64($0.uint64Value) }
    }

    func windowDescriptions(for windowIDs: [UInt64]) throws -> [SystemWindowDescription] {
        let descriptions = try rawWindowDescriptions(for: windowIDs)
        return descriptions.map(parseWindowDescription)
    }

    func onscreenWindowOrder() throws -> [UInt64] {
        guard let payload = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] else {
            throw BackendError.missingTopology("CGWindowListCopyWindowInfo")
        }

        return payload.compactMap { u64($0[kCGWindowNumber as String]) }
    }

    func focusedWindowID() throws -> UInt64? {
        typealias Function = @convention(c) (AXUIElement, UnsafeMutablePointer<CGWindowID>) -> AXError
        guard let function = symbols.resolve("_AXUIElementGetWindow", as: Function.self) else {
            throw BackendError.missingTopology("_AXUIElementGetWindow")
        }

        let systemWide = AXUIElementCreateSystemWide()
        guard let application = try copyAXElementAttribute(
            from: systemWide,
            attribute: kAXFocusedApplicationAttribute as CFString
        ) else {
            return nil
        }
        guard let window = try copyAXElementAttribute(
            from: application,
            attribute: kAXFocusedWindowAttribute as CFString
        ) else {
            return nil
        }

        var windowID: CGWindowID = 0
        guard function(window, &windowID) == .success, windowID != 0 else {
            return nil
        }

        return UInt64(windowID)
    }

    func stableAppID(for pid: UInt32) -> String? {
        NSRunningApplication(processIdentifier: pid_t(pid))?.bundleIdentifier
    }

    private func rawWindowDescriptions(for windowIDs: [UInt64]) throws -> [[String: Any]] {
        let cfWindowIDs = windowIDs.map { NSNumber(value: $0) } as CFArray
        let direct = CGWindowListCreateDescriptionFromArray(cfWindowIDs) as? [[String: Any]]
        if let direct, !direct.isEmpty {
            return direct
        }

        let targetIDs = Set(windowIDs)
        guard let onscreen = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] else {
            throw BackendError.missingTopology("CGWindowListCopyWindowInfo")
        }

        return onscreen.filter { description in
            guard let windowID = u64(description[kCGWindowNumber as String]) else {
                return false
            }
            return targetIDs.contains(windowID)
        }
    }

    private func parseWindowDescription(_ description: [String: Any]) -> SystemWindowDescription {
        let id = u64(description[kCGWindowNumber as String]) ?? 0
        let pid = u32(description[kCGWindowOwnerPID as String])
        let title = description[kCGWindowName as String] as? String
        let level = i32(description[kCGWindowLayer as String]) ?? 0
        let frame = cgBounds(from: description[kCGWindowBounds as String])

        return SystemWindowDescription(
            id: id,
            pid: pid,
            appID: nil,
            title: title,
            level: level,
            visibleIndex: nil,
            frame: frame
        )
    }

    private func copyAXElementAttribute(from element: AXUIElement, attribute: CFString) throws -> AXUIElement? {
        var value: CFTypeRef?
        let error = AXUIElementCopyAttributeValue(element, attribute, &value)
        switch error {
        case .success:
            return value.map { unsafeBitCast($0, to: AXUIElement.self) }
        case .attributeUnsupported, .noValue:
            return nil
        default:
            throw BackendError.missingTopology("AXUIElementCopyAttributeValue")
        }
    }
}
