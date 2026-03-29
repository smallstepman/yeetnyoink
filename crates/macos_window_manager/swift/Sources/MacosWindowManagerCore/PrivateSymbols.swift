import Darwin
import Foundation

public enum PrivateSymbols {
    public static let requiredSymbols = [
        "SLSMainConnectionID",
        "SLSCopyManagedDisplaySpaces",
        "SLSManagedDisplayGetCurrentSpace",
        "SLSManagedDisplaySetCurrentSpace",
        "SLSCopyManagedDisplayForSpace",
        "SLSCopyWindowsWithOptionsAndTags",
        "SLSMoveWindowsToManagedSpace",
        "AXIsProcessTrusted",
        "_AXUIElementGetWindow",
        "_SLPSSetFrontProcessWithOptions",
        "GetProcessForPID",
    ]

    static let frameworkPaths = [
        "/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight",
        "/System/Library/Frameworks/ApplicationServices.framework/Frameworks/HIServices.framework/HIServices",
    ]
}

final class PrivateSymbolResolver {
    private let handles: [UnsafeMutableRawPointer]

    init() {
        self.handles = PrivateSymbols.frameworkPaths.compactMap { dlopen($0, RTLD_LAZY) }
    }

    deinit {
        handles.forEach { dlclose($0) }
    }

    func hasSymbol(_ name: String) -> Bool {
        resolveRaw(name) != nil
    }

    func resolve<T>(_ name: String, as type: T.Type = T.self) -> T? {
        guard let raw = resolveRaw(name) else {
            return nil
        }

        return unsafeBitCast(raw, to: T.self)
    }

    private func resolveRaw(_ name: String) -> UnsafeMutableRawPointer? {
        handles.lazy.compactMap { handle in dlsym(handle, name) }.first
    }
}
