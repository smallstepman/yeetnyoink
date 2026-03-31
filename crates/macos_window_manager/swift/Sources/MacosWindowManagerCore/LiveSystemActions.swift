import ApplicationServices
import CoreGraphics
import Foundation

private let cpsUserGenerated: UInt32 = 0x200
private let axRaiseSettleTimeout: TimeInterval = 0.3
private let axRaiseRetryInterval: useconds_t = 10_000
private let westAdjacentSpaceKeyCode: CGKeyCode = 0x7B
private let eastAdjacentSpaceKeyCode: CGKeyCode = 0x7C
private let missionControlHotkeyFlags = CGEventFlags(rawValue: (1 << 18) | (1 << 23))

private struct WindowServerProcessSerialNumber {
    var highLongOfPSN: UInt32 = 0
    var lowLongOfPSN: UInt32 = 0
}

extension LiveSystem: BackendActionSystem {
    func switchSpace(_ spaceID: UInt64) throws {
        typealias CopyManagedDisplayForSpace = @convention(c) (UInt32, UInt64) -> Unmanaged<CFString>?
        typealias SetCurrentSpace = @convention(c) (UInt32, CFString, UInt64) -> Void

        guard let copyManagedDisplayForSpace = resolveSymbol(
            "SLSCopyManagedDisplayForSpace",
            as: CopyManagedDisplayForSpace.self
        ) else {
            throw BackendOperationError.callFailed("SLSCopyManagedDisplayForSpace")
        }
        guard let setCurrentSpace = resolveSymbol(
            "SLSManagedDisplaySetCurrentSpace",
            as: SetCurrentSpace.self
        ) else {
            throw BackendOperationError.callFailed("SLSManagedDisplaySetCurrentSpace")
        }
        guard let connectionID = mainConnectionID() else {
            throw BackendOperationError.callFailed("SLSMainConnectionID")
        }
        guard let displayIdentifier = copyManagedDisplayForSpace(connectionID, spaceID)?.takeRetainedValue() else {
            throw BackendOperationError.callFailed("SLSCopyManagedDisplayForSpace")
        }

        setCurrentSpace(connectionID, displayIdentifier, spaceID)
    }

    func switchAdjacentSpace(_ direction: NativeDirection, targetSpaceID: UInt64) throws {
        _ = targetSpaceID
        let keyCode = try adjacentSpaceHotkey(direction)
        try postKeyboardEvent(keyCode: keyCode, keyDown: true, flags: missionControlHotkeyFlags)
        try postKeyboardEvent(keyCode: keyCode, keyDown: false, flags: missionControlHotkeyFlags)
    }

    func focusWindow(_ windowID: UInt64) throws {
        let window = try windowDescription(id: windowID)
        guard let pid = window.pid else {
            throw BackendOperationError.missingWindowPID(windowID)
        }

        try focusWindow(windowID, pid: pid, frontsProcess: true)
    }

    func focusWindowWithKnownPID(_ windowID: UInt64, pid: UInt32) throws {
        try focusWindow(windowID, pid: pid, frontsProcess: true)
    }

    func axWindowIDs(for pid: UInt32) throws -> [UInt64] {
        guard let windows = axWindows(for: pid) else {
            return []
        }

        return windows.compactMap { try? axWindowID(for: $0) }
    }

    func moveWindowToSpace(_ windowID: UInt64, spaceID: UInt64) throws {
        typealias MoveWindowsToManagedSpace = @convention(c) (UInt32, CFArray, UInt64) -> Void

        guard let moveWindowsToManagedSpace = resolveSymbol(
            "SLSMoveWindowsToManagedSpace",
            as: MoveWindowsToManagedSpace.self
        ) else {
            throw BackendOperationError.callFailed("SLSMoveWindowsToManagedSpace")
        }
        guard let connectionID = mainConnectionID() else {
            throw BackendOperationError.callFailed("SLSMainConnectionID")
        }

        let windowList = [NSNumber(value: windowID)] as CFArray
        moveWindowsToManagedSpace(connectionID, windowList, spaceID)
    }

    func swapWindowFrames(
        sourceWindowID: UInt64,
        sourceFrame: NativeBounds,
        targetWindowID: UInt64,
        targetFrame: NativeBounds
    ) throws {
        let sourceWindow = try windowDescription(id: sourceWindowID)
        guard let sourcePID = sourceWindow.pid else {
            throw BackendOperationError.missingWindowPID(sourceWindowID)
        }
        let targetWindow = try windowDescription(id: targetWindowID)
        guard let targetPID = targetWindow.pid else {
            throw BackendOperationError.missingWindowPID(targetWindowID)
        }

        try setWindowFrame(windowID: sourceWindowID, pid: sourcePID, frame: targetFrame)
        try setWindowFrame(windowID: targetWindowID, pid: targetPID, frame: sourceFrame)
    }
}

private extension LiveSystem {
    func focusWindow(_ windowID: UInt64, pid: UInt32, frontsProcess: Bool) throws {
        let processSerialNumber = try processSerialNumber(for: pid)

        do {
            if frontsProcess {
                try frontProcessWindow(processSerialNumber, windowID: windowID)
            }
            try makeKeyWindow(processSerialNumber, windowID: windowID)
            try raiseWindowWithRetry(windowID, pid: pid)
        } catch let error as BackendOperationError {
            guard case .missingWindow(let missingWindowID) = error, missingWindowID == windowID else {
                throw error
            }
            guard confirmFocusAfterMissingAXTarget(windowID) else {
                throw error
            }
        }
    }

    func windowDescription(id windowID: UInt64) throws -> SystemWindowDescription {
        do {
            guard let window = try windowDescriptions(for: [windowID]).first(where: { $0.id == windowID }) else {
                throw BackendOperationError.missingWindow(windowID)
            }
            return window
        } catch let error as BackendOperationError {
            throw error
        } catch let error as BackendError {
            throw operationError(for: error)
        } catch {
            throw BackendOperationError.callFailed("CGWindowListCopyWindowInfo")
        }
    }

    func operationError(for error: BackendError) -> BackendOperationError {
        switch error {
        case let .missingRequiredSymbol(symbol):
            .callFailed(symbol)
        case .missingAccessibilityPermission:
            .callFailed("AXIsProcessTrusted")
        case let .missingTopologyPrecondition(precondition):
            .callFailed(precondition)
        case let .missingTopology(probe):
            .callFailed(probe)
        }
    }

    func processSerialNumber(for pid: UInt32) throws -> WindowServerProcessSerialNumber {
        typealias GetProcessForPID = @convention(c) (Int32, UnsafeMutableRawPointer) -> Int32

        guard let getProcessForPID = resolveSymbol(
            "GetProcessForPID",
            as: GetProcessForPID.self
        ) else {
            throw BackendOperationError.callFailed("GetProcessForPID")
        }

        var processSerialNumber = WindowServerProcessSerialNumber()
        guard withUnsafeMutablePointer(to: &processSerialNumber, { processSerialNumber in
            getProcessForPID(Int32(pid), UnsafeMutableRawPointer(processSerialNumber))
        }) == 0 else {
            throw BackendOperationError.callFailed("GetProcessForPID")
        }

        return processSerialNumber
    }

    func frontProcessWindow(
        _ processSerialNumber: WindowServerProcessSerialNumber,
        windowID: UInt64
    ) throws {
        typealias SetFrontProcessWithOptions = @convention(c) (
            UnsafeRawPointer,
            UInt32,
            UInt32
        ) -> Int32

        guard let setFrontProcessWithOptions = resolveSymbol(
            "_SLPSSetFrontProcessWithOptions",
            as: SetFrontProcessWithOptions.self
        ) else {
            throw BackendOperationError.callFailed("_SLPSSetFrontProcessWithOptions")
        }
        guard let windowID = UInt32(exactly: windowID) else {
            throw BackendOperationError.missingWindow(windowID)
        }

        var processSerialNumber = processSerialNumber
        guard withUnsafePointer(to: &processSerialNumber, { processSerialNumber in
            setFrontProcessWithOptions(UnsafeRawPointer(processSerialNumber), windowID, cpsUserGenerated)
        }) == 0 else {
            throw BackendOperationError.callFailed("_SLPSSetFrontProcessWithOptions")
        }
    }

    func makeKeyWindow(
        _ processSerialNumber: WindowServerProcessSerialNumber,
        windowID: UInt64
    ) throws {
        typealias PostEventRecordTo = @convention(c) (
            UnsafeRawPointer,
            UnsafeRawPointer
        ) -> Int32

        guard let postEventRecordTo = resolveSymbol(
            "SLPSPostEventRecordTo",
            as: PostEventRecordTo.self
        ) else {
            throw BackendOperationError.callFailed("SLPSPostEventRecordTo")
        }
        guard let windowID = UInt32(exactly: windowID) else {
            throw BackendOperationError.missingWindow(windowID)
        }

        var processSerialNumber = processSerialNumber
        var eventBytes = [UInt8](repeating: 0, count: 0xF8)
        eventBytes[0x04] = 0xF8
        eventBytes[0x3A] = 0x10
        eventBytes.replaceSubrange(0x3C ..< 0x40, with: windowID.littleEndian.bytes[0 ..< 4])
        eventBytes.replaceSubrange(0x20 ..< 0x30, with: Array<UInt8>(repeating: 0xFF, count: 0x10))

        eventBytes[0x08] = 0x01
        let pressStatus = withUnsafePointer(to: &processSerialNumber) { processSerialNumber in
            eventBytes.withUnsafeBytes { bytes in
                postEventRecordTo(UnsafeRawPointer(processSerialNumber), bytes.baseAddress!)
            }
        }
        guard pressStatus == 0 else {
            throw BackendOperationError.callFailed("SLPSPostEventRecordTo")
        }

        eventBytes[0x08] = 0x02
        let releaseStatus = withUnsafePointer(to: &processSerialNumber) { processSerialNumber in
            eventBytes.withUnsafeBytes { bytes in
                postEventRecordTo(UnsafeRawPointer(processSerialNumber), bytes.baseAddress!)
            }
        }
        guard releaseStatus == 0 else {
            throw BackendOperationError.callFailed("SLPSPostEventRecordTo")
        }
    }

    func raiseWindowWithRetry(_ windowID: UInt64, pid: UInt32) throws {
        let deadline = Date().addingTimeInterval(axRaiseSettleTimeout)

        while true {
            do {
                try raiseWindowViaAX(windowID, pid: pid)
                return
            } catch let error as BackendOperationError {
                guard case .missingWindow(let missingWindowID) = error,
                      missingWindowID == windowID,
                      Date() < deadline
                else {
                    throw error
                }

                usleep(axRaiseRetryInterval)
            }
        }
    }

    func confirmFocusAfterMissingAXTarget(_ windowID: UInt64) -> Bool {
        let deadline = Date().addingTimeInterval(axRaiseSettleTimeout)

        while true {
            if (try? focusedWindowID()) == Optional(windowID) {
                return true
            }
            if Date() >= deadline {
                return false
            }

            usleep(axRaiseRetryInterval)
        }
    }

    func adjacentSpaceHotkey(_ direction: NativeDirection) throws -> CGKeyCode {
        switch direction {
        case .west:
            westAdjacentSpaceKeyCode
        case .east:
            eastAdjacentSpaceKeyCode
        case .north, .south:
            throw BackendOperationError.callFailed("adjacent_space_hotkey_direction")
        }
    }

    func postKeyboardEvent(keyCode: CGKeyCode, keyDown: Bool, flags: CGEventFlags) throws {
        guard let event = CGEvent(
            keyboardEventSource: nil,
            virtualKey: keyCode,
            keyDown: keyDown
        ) else {
            throw BackendOperationError.callFailed("CGEventCreateKeyboardEvent")
        }

        event.flags = flags
        event.post(tap: .cghidEventTap)
    }

    func raiseWindowViaAX(_ windowID: UInt64, pid: UInt32) throws {
        let window = try copyWindowAXElement(for: pid, windowID: windowID)
        guard AXUIElementPerformAction(window, kAXRaiseAction as CFString) == .success else {
            throw BackendOperationError.callFailed("AXUIElementPerformAction")
        }
    }

    func setWindowFrame(windowID: UInt64, pid: UInt32, frame: NativeBounds) throws {
        let window = try copyWindowAXElement(for: pid, windowID: windowID)

        var position = CGPoint(x: CGFloat(frame.x), y: CGFloat(frame.y))
        guard let positionValue = AXValueCreate(.cgPoint, &position) else {
            throw BackendOperationError.callFailed("AXValueCreate")
        }
        guard AXUIElementSetAttributeValue(window, kAXPositionAttribute as CFString, positionValue) == .success else {
            throw BackendOperationError.callFailed("AXUIElementSetAttributeValue")
        }

        var size = CGSize(width: CGFloat(frame.width), height: CGFloat(frame.height))
        guard let sizeValue = AXValueCreate(.cgSize, &size) else {
            throw BackendOperationError.callFailed("AXValueCreate")
        }
        guard AXUIElementSetAttributeValue(window, kAXSizeAttribute as CFString, sizeValue) == .success else {
            throw BackendOperationError.callFailed("AXUIElementSetAttributeValue")
        }
    }

    func axWindows(for pid: UInt32) -> [AXUIElement]? {
        let application = AXUIElementCreateApplication(pid_t(pid))
        guard let rawWindows = copyAXAttributeValue(from: application, attribute: kAXWindowsAttribute as CFString) else {
            return nil
        }
        if let windows = rawWindows as? [AXUIElement] {
            return windows
        }
        if let windows = rawWindows as? NSArray {
            return windows.map { unsafeBitCast($0, to: AXUIElement.self) }
        }
        return nil
    }

    func copyWindowAXElement(for pid: UInt32, windowID: UInt64) throws -> AXUIElement {
        guard let windows = axWindows(for: pid) else {
            throw BackendOperationError.missingWindow(windowID)
        }

        for window in windows {
            guard let candidateWindowID = try? axWindowID(for: window) else {
                continue
            }
            if candidateWindowID == windowID {
                return window
            }
        }

        throw BackendOperationError.missingWindow(windowID)
    }

    func axWindowID(for element: AXUIElement) throws -> UInt64 {
        typealias AXUIElementGetWindow = @convention(c) (AXUIElement, UnsafeMutablePointer<CGWindowID>) -> AXError

        guard let axUIElementGetWindow = resolveSymbol(
            "_AXUIElementGetWindow",
            as: AXUIElementGetWindow.self
        ) else {
            throw BackendOperationError.callFailed("_AXUIElementGetWindow")
        }

        var windowID: CGWindowID = 0
        guard axUIElementGetWindow(element, &windowID) == .success, windowID != 0 else {
            throw BackendOperationError.missingWindow(UInt64(windowID))
        }

        return UInt64(windowID)
    }

    func copyAXAttributeValue(from element: AXUIElement, attribute: CFString) -> CFTypeRef? {
        var value: CFTypeRef?
        guard AXUIElementCopyAttributeValue(element, attribute, &value) == .success else {
            return nil
        }

        return value
    }
}

private extension FixedWidthInteger {
    var bytes: [UInt8] {
        withUnsafeBytes(of: self) { Array($0) }
    }
}
