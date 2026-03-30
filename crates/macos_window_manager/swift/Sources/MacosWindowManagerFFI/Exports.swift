import MacosWindowManagerCore

private let MWM_STATUS_OPERATION_MISSING_SPACE: Int32 = 30
private let MWM_STATUS_OPERATION_MISSING_WINDOW: Int32 = 31
private let MWM_STATUS_OPERATION_MISSING_WINDOW_FRAME: Int32 = 32
private let MWM_STATUS_OPERATION_MISSING_WINDOW_PID: Int32 = 33
private let MWM_STATUS_OPERATION_UNSUPPORTED_STAGE_MANAGER_SPACE: Int32 = 34
private let MWM_STATUS_OPERATION_NO_DIRECTIONAL_FOCUS_TARGET: Int32 = 35
private let MWM_STATUS_OPERATION_NO_DIRECTIONAL_MOVE_TARGET: Int32 = 36
private let MWM_STATUS_OPERATION_CALL_FAILED: Int32 = 37

private final class BackendHandle {
    private let backend = Backend()

    func validateEnvironment() throws {
        try backend.validateEnvironment()
    }

    func desktopSnapshot() throws -> MwmDesktopSnapshotAbi {
        MwmDesktopSnapshotAbi(try backend.desktopSnapshot())
    }

    func topologySnapshot() throws -> MwmDesktopSnapshotAbi {
        MwmDesktopSnapshotAbi(try backend.topologySnapshot())
    }

    func switchSpace(_ spaceID: UInt64) throws {
        try backend.switchSpace(spaceID)
    }

    func switchAdjacentSpace(_ direction: NativeDirection, targetSpaceID: UInt64) throws {
        try backend.switchAdjacentSpace(direction, targetSpaceID: targetSpaceID)
    }

    func switchSpaceInSnapshot(
        snapshot: DesktopSnapshot,
        targetSpaceID: UInt64,
        adjacentDirection: NativeDirection?
    ) throws {
        try backend.switchSpaceInSnapshot(
            snapshot: snapshot,
            targetSpaceID: targetSpaceID,
            adjacentDirection: adjacentDirection
        )
    }

    func focusWindow(_ windowID: UInt64) throws {
        try backend.focusWindow(windowID)
    }

    func focusWindowWithKnownPID(_ windowID: UInt64, pid: UInt32) throws {
        try backend.focusWindowWithKnownPID(windowID, pid: pid)
    }

    func focusWindowInActiveSpaceWithKnownPID(
        _ windowID: UInt64,
        pid: UInt32,
        targetHint: ActiveSpaceFocusTargetHint?
    ) throws {
        try backend.focusWindowInActiveSpaceWithKnownPID(windowID, pid: pid, targetHint: targetHint)
    }

    func focusSameSpaceTargetInSnapshot(
        snapshot: DesktopSnapshot,
        direction: NativeDirection,
        targetWindowID: UInt64
    ) throws {
        try backend.focusSameSpaceTargetInSnapshot(
            snapshot: snapshot,
            direction: direction,
            targetWindowID: targetWindowID
        )
    }

    func moveWindowToSpace(_ windowID: UInt64, spaceID: UInt64) throws {
        try backend.moveWindowToSpace(windowID, spaceID: spaceID)
    }

    func swapWindowFrames(
        sourceWindowID: UInt64,
        sourceFrame: NativeBounds,
        targetWindowID: UInt64,
        targetFrame: NativeBounds
    ) throws {
        try backend.swapWindowFrames(
            sourceWindowID: sourceWindowID,
            sourceFrame: sourceFrame,
            targetWindowID: targetWindowID,
            targetFrame: targetFrame
        )
    }
}

@inline(__always)
private func writeStatus(
    _ outStatus: UnsafeMutableRawPointer?,
    code: Int32,
    message: UnsafeMutablePointer<CChar>? = nil
) {
    guard let outStatus else {
        return
    }

    outStatus
        .assumingMemoryBound(to: MwmStatus.self)
        .pointee = MwmStatus(code: code, message_ptr: message)
}

@inline(__always)
private func writeErrorStatus(_ outStatus: UnsafeMutableRawPointer?, error: BackendError) -> Int32 {
    let code: Int32
    let message: String?

    switch error {
    case let .missingRequiredSymbol(symbol):
        code = MWM_STATUS_CONNECT_MISSING_REQUIRED_SYMBOL
        message = symbol
    case .missingAccessibilityPermission:
        code = MWM_STATUS_CONNECT_MISSING_ACCESSIBILITY_PERMISSION
        message = nil
    case let .missingTopologyPrecondition(precondition):
        code = MWM_STATUS_CONNECT_MISSING_TOPOLOGY_PRECONDITION
        message = precondition
    case let .missingTopology(probe):
        code = MWM_STATUS_PROBE_MISSING_TOPOLOGY
        message = probe
    }

    writeStatus(outStatus, code: code, message: message?.ownedCString())
    return code
}

@inline(__always)
private func writeOperationErrorStatus(
    _ outStatus: UnsafeMutableRawPointer?,
    error: BackendOperationError
) -> Int32 {
    let code: Int32
    let message: String?

    switch error {
    case let .missingSpace(spaceID):
        code = MWM_STATUS_OPERATION_MISSING_SPACE
        message = String(spaceID)
    case let .missingWindow(windowID):
        code = MWM_STATUS_OPERATION_MISSING_WINDOW
        message = String(windowID)
    case let .missingWindowFrame(windowID):
        code = MWM_STATUS_OPERATION_MISSING_WINDOW_FRAME
        message = String(windowID)
    case let .missingWindowPID(windowID):
        code = MWM_STATUS_OPERATION_MISSING_WINDOW_PID
        message = String(windowID)
    case let .unsupportedStageManagerSpace(spaceID):
        code = MWM_STATUS_OPERATION_UNSUPPORTED_STAGE_MANAGER_SPACE
        message = String(spaceID)
    case let .noDirectionalFocusTarget(direction):
        code = MWM_STATUS_OPERATION_NO_DIRECTIONAL_FOCUS_TARGET
        message = operationDirection(direction)
    case let .noDirectionalMoveTarget(direction):
        code = MWM_STATUS_OPERATION_NO_DIRECTIONAL_MOVE_TARGET
        message = operationDirection(direction)
    case let .callFailed(operation):
        code = MWM_STATUS_OPERATION_CALL_FAILED
        message = operation
    }

    writeStatus(outStatus, code: code, message: message?.ownedCString())
    return code
}

@_cdecl("mwm_backend_smoke_test")
public func mwm_backend_smoke_test() -> Int32 {
    Backend.smokeTest()
}

@_cdecl("mwm_backend_new")
public func mwm_backend_new(
    _ outBackend: UnsafeMutablePointer<UnsafeMutableRawPointer?>?,
    _ outStatus: UnsafeMutableRawPointer?
) -> Int32 {
    verifyTransportAbiContract()

    guard let outBackend else {
        writeStatus(outStatus, code: MWM_STATUS_INVALID_ARGUMENT)
        return MWM_STATUS_INVALID_ARGUMENT
    }

    outBackend.pointee = Unmanaged.passRetained(BackendHandle()).toOpaque()
    writeStatus(outStatus, code: MWM_STATUS_OK)
    return MWM_STATUS_OK
}

@_cdecl("mwm_backend_validate_environment")
public func mwm_backend_validate_environment(
    _ backend: UnsafeMutableRawPointer?,
    _ outStatus: UnsafeMutableRawPointer?
) -> Int32 {
    verifyTransportAbiContract()

    guard let backend else {
        writeStatus(outStatus, code: MWM_STATUS_INVALID_ARGUMENT)
        return MWM_STATUS_INVALID_ARGUMENT
    }

    let handle = Unmanaged<BackendHandle>.fromOpaque(backend).takeUnretainedValue()
    do {
        try handle.validateEnvironment()
        writeStatus(outStatus, code: MWM_STATUS_OK)
        return MWM_STATUS_OK
    } catch let error as BackendError {
        return writeErrorStatus(outStatus, error: error)
    } catch {
        writeStatus(outStatus, code: MWM_STATUS_UNAVAILABLE, message: String(describing: error).ownedCString())
        return MWM_STATUS_UNAVAILABLE
    }
}

@_cdecl("mwm_backend_free")
public func mwm_backend_free(_ backend: UnsafeMutableRawPointer?) {
    verifyTransportAbiContract()

    guard let backend else {
        return
    }

    Unmanaged<BackendHandle>.fromOpaque(backend).release()
}

@_cdecl("mwm_backend_desktop_snapshot")
public func mwm_backend_desktop_snapshot(
    _ backend: UnsafeMutableRawPointer?,
    _ outSnapshot: UnsafeMutableRawPointer?,
    _ outStatus: UnsafeMutableRawPointer?
) -> Int32 {
    verifyTransportAbiContract()

    guard let backend, let outSnapshot else {
        writeStatus(outStatus, code: MWM_STATUS_INVALID_ARGUMENT)
        return MWM_STATUS_INVALID_ARGUMENT
    }

    let handle = Unmanaged<BackendHandle>.fromOpaque(backend).takeUnretainedValue()
    do {
        outSnapshot
            .assumingMemoryBound(to: MwmDesktopSnapshotAbi.self)
            .pointee = try handle.desktopSnapshot()
        writeStatus(outStatus, code: MWM_STATUS_OK)
        return MWM_STATUS_OK
    } catch let error as BackendError {
        return writeErrorStatus(outStatus, error: error)
    } catch {
        writeStatus(outStatus, code: MWM_STATUS_UNAVAILABLE, message: String(describing: error).ownedCString())
        return MWM_STATUS_UNAVAILABLE
    }
}

@_cdecl("mwm_backend_topology_snapshot")
public func mwm_backend_topology_snapshot(
    _ backend: UnsafeMutableRawPointer?,
    _ outSnapshot: UnsafeMutableRawPointer?,
    _ outStatus: UnsafeMutableRawPointer?
) -> Int32 {
    verifyTransportAbiContract()

    guard let backend, let outSnapshot else {
        writeStatus(outStatus, code: MWM_STATUS_INVALID_ARGUMENT)
        return MWM_STATUS_INVALID_ARGUMENT
    }

    let handle = Unmanaged<BackendHandle>.fromOpaque(backend).takeUnretainedValue()
    do {
        outSnapshot
            .assumingMemoryBound(to: MwmDesktopSnapshotAbi.self)
            .pointee = try handle.topologySnapshot()
        writeStatus(outStatus, code: MWM_STATUS_OK)
        return MWM_STATUS_OK
    } catch let error as BackendError {
        return writeErrorStatus(outStatus, error: error)
    } catch {
        writeStatus(outStatus, code: MWM_STATUS_UNAVAILABLE, message: String(describing: error).ownedCString())
        return MWM_STATUS_UNAVAILABLE
    }
}

@_cdecl("mwm_backend_switch_space")
public func mwm_backend_switch_space(
    _ backend: UnsafeMutableRawPointer?,
    _ spaceID: UInt64,
    _ outStatus: UnsafeMutableRawPointer?
) -> Int32 {
    verifyTransportAbiContract()

    guard let backend else {
        writeStatus(outStatus, code: MWM_STATUS_INVALID_ARGUMENT)
        return MWM_STATUS_INVALID_ARGUMENT
    }

    let handle = Unmanaged<BackendHandle>.fromOpaque(backend).takeUnretainedValue()
    do {
        try handle.switchSpace(spaceID)
        writeStatus(outStatus, code: MWM_STATUS_OK)
        return MWM_STATUS_OK
    } catch let error as BackendError {
        return writeErrorStatus(outStatus, error: error)
    } catch let error as BackendOperationError {
        return writeOperationErrorStatus(outStatus, error: error)
    } catch {
        writeStatus(outStatus, code: MWM_STATUS_UNAVAILABLE, message: String(describing: error).ownedCString())
        return MWM_STATUS_UNAVAILABLE
    }
}

@_cdecl("mwm_backend_switch_adjacent_space")
public func mwm_backend_switch_adjacent_space(
    _ backend: UnsafeMutableRawPointer?,
    _ directionRaw: Int32,
    _ targetSpaceID: UInt64,
    _ outStatus: UnsafeMutableRawPointer?
) -> Int32 {
    verifyTransportAbiContract()

    guard let backend, let direction = NativeDirection(rawValue: directionRaw) else {
        writeStatus(outStatus, code: MWM_STATUS_INVALID_ARGUMENT)
        return MWM_STATUS_INVALID_ARGUMENT
    }

    let handle = Unmanaged<BackendHandle>.fromOpaque(backend).takeUnretainedValue()
    do {
        try handle.switchAdjacentSpace(direction, targetSpaceID: targetSpaceID)
        writeStatus(outStatus, code: MWM_STATUS_OK)
        return MWM_STATUS_OK
    } catch let error as BackendError {
        return writeErrorStatus(outStatus, error: error)
    } catch let error as BackendOperationError {
        return writeOperationErrorStatus(outStatus, error: error)
    } catch {
        writeStatus(outStatus, code: MWM_STATUS_UNAVAILABLE, message: String(describing: error).ownedCString())
        return MWM_STATUS_UNAVAILABLE
    }
}

@_cdecl("mwm_backend_switch_space_in_snapshot")
public func mwm_backend_switch_space_in_snapshot(
    _ backend: UnsafeMutableRawPointer?,
    _ snapshot: UnsafeMutableRawPointer?,
    _ targetSpaceID: UInt64,
    _ adjacentDirectionRaw: Int32,
    _ outStatus: UnsafeMutableRawPointer?
) -> Int32 {
    verifyTransportAbiContract()

    guard let backend, let snapshot else {
        writeStatus(outStatus, code: MWM_STATUS_INVALID_ARGUMENT)
        return MWM_STATUS_INVALID_ARGUMENT
    }

    let handle = Unmanaged<BackendHandle>.fromOpaque(backend).takeUnretainedValue()
    let adjacentDirection = adjacentDirectionRaw >= 0 ? NativeDirection(rawValue: adjacentDirectionRaw) : nil
    if adjacentDirectionRaw >= 0, adjacentDirection == nil {
        writeStatus(outStatus, code: MWM_STATUS_INVALID_ARGUMENT)
        return MWM_STATUS_INVALID_ARGUMENT
    }

    do {
        try handle.switchSpaceInSnapshot(
            snapshot: DesktopSnapshot(snapshotABI: snapshot.assumingMemoryBound(to: MwmDesktopSnapshotAbi.self).pointee),
            targetSpaceID: targetSpaceID,
            adjacentDirection: adjacentDirection
        )
        writeStatus(outStatus, code: MWM_STATUS_OK)
        return MWM_STATUS_OK
    } catch let error as BackendError {
        return writeErrorStatus(outStatus, error: error)
    } catch let error as BackendOperationError {
        return writeOperationErrorStatus(outStatus, error: error)
    } catch {
        writeStatus(outStatus, code: MWM_STATUS_UNAVAILABLE, message: String(describing: error).ownedCString())
        return MWM_STATUS_UNAVAILABLE
    }
}

@_cdecl("mwm_backend_focus_window")
public func mwm_backend_focus_window(
    _ backend: UnsafeMutableRawPointer?,
    _ windowID: UInt64,
    _ outStatus: UnsafeMutableRawPointer?
) -> Int32 {
    verifyTransportAbiContract()

    guard let backend else {
        writeStatus(outStatus, code: MWM_STATUS_INVALID_ARGUMENT)
        return MWM_STATUS_INVALID_ARGUMENT
    }

    let handle = Unmanaged<BackendHandle>.fromOpaque(backend).takeUnretainedValue()
    do {
        try handle.focusWindow(windowID)
        writeStatus(outStatus, code: MWM_STATUS_OK)
        return MWM_STATUS_OK
    } catch let error as BackendError {
        return writeErrorStatus(outStatus, error: error)
    } catch let error as BackendOperationError {
        return writeOperationErrorStatus(outStatus, error: error)
    } catch {
        writeStatus(outStatus, code: MWM_STATUS_UNAVAILABLE, message: String(describing: error).ownedCString())
        return MWM_STATUS_UNAVAILABLE
    }
}

@_cdecl("mwm_backend_focus_window_with_known_pid")
public func mwm_backend_focus_window_with_known_pid(
    _ backend: UnsafeMutableRawPointer?,
    _ windowID: UInt64,
    _ pid: UInt32,
    _ outStatus: UnsafeMutableRawPointer?
) -> Int32 {
    verifyTransportAbiContract()

    guard let backend else {
        writeStatus(outStatus, code: MWM_STATUS_INVALID_ARGUMENT)
        return MWM_STATUS_INVALID_ARGUMENT
    }

    let handle = Unmanaged<BackendHandle>.fromOpaque(backend).takeUnretainedValue()
    do {
        try handle.focusWindowWithKnownPID(windowID, pid: pid)
        writeStatus(outStatus, code: MWM_STATUS_OK)
        return MWM_STATUS_OK
    } catch let error as BackendError {
        return writeErrorStatus(outStatus, error: error)
    } catch let error as BackendOperationError {
        return writeOperationErrorStatus(outStatus, error: error)
    } catch {
        writeStatus(outStatus, code: MWM_STATUS_UNAVAILABLE, message: String(describing: error).ownedCString())
        return MWM_STATUS_UNAVAILABLE
    }
}

@_cdecl("mwm_backend_focus_window_in_active_space_with_known_pid")
public func mwm_backend_focus_window_in_active_space_with_known_pid(
    _ backend: UnsafeMutableRawPointer?,
    _ windowID: UInt64,
    _ pid: UInt32,
    _ hasTargetHint: UInt8,
    _ targetHintSpaceID: UInt64,
    _ targetHintX: Int32,
    _ targetHintY: Int32,
    _ targetHintWidth: Int32,
    _ targetHintHeight: Int32,
    _ outStatus: UnsafeMutableRawPointer?
) -> Int32 {
    verifyTransportAbiContract()

    guard let backend else {
        writeStatus(outStatus, code: MWM_STATUS_INVALID_ARGUMENT)
        return MWM_STATUS_INVALID_ARGUMENT
    }

    let handle = Unmanaged<BackendHandle>.fromOpaque(backend).takeUnretainedValue()
    let targetHint = hasTargetHint == 0 ? nil : ActiveSpaceFocusTargetHint(
        spaceID: targetHintSpaceID,
        bounds: NativeBounds(
            x: targetHintX,
            y: targetHintY,
            width: targetHintWidth,
            height: targetHintHeight
        )
    )
    do {
        try handle.focusWindowInActiveSpaceWithKnownPID(windowID, pid: pid, targetHint: targetHint)
        writeStatus(outStatus, code: MWM_STATUS_OK)
        return MWM_STATUS_OK
    } catch let error as BackendError {
        return writeErrorStatus(outStatus, error: error)
    } catch let error as BackendOperationError {
        return writeOperationErrorStatus(outStatus, error: error)
    } catch {
        writeStatus(outStatus, code: MWM_STATUS_UNAVAILABLE, message: String(describing: error).ownedCString())
        return MWM_STATUS_UNAVAILABLE
    }
}

@_cdecl("mwm_backend_focus_same_space_target_in_snapshot")
public func mwm_backend_focus_same_space_target_in_snapshot(
    _ backend: UnsafeMutableRawPointer?,
    _ snapshot: UnsafeMutableRawPointer?,
    _ directionRaw: Int32,
    _ targetWindowID: UInt64,
    _ outStatus: UnsafeMutableRawPointer?
) -> Int32 {
    verifyTransportAbiContract()

    guard let backend, let snapshot, let direction = NativeDirection(rawValue: directionRaw) else {
        writeStatus(outStatus, code: MWM_STATUS_INVALID_ARGUMENT)
        return MWM_STATUS_INVALID_ARGUMENT
    }

    let handle = Unmanaged<BackendHandle>.fromOpaque(backend).takeUnretainedValue()
    do {
        try handle.focusSameSpaceTargetInSnapshot(
            snapshot: DesktopSnapshot(snapshotABI: snapshot.assumingMemoryBound(to: MwmDesktopSnapshotAbi.self).pointee),
            direction: direction,
            targetWindowID: targetWindowID
        )
        writeStatus(outStatus, code: MWM_STATUS_OK)
        return MWM_STATUS_OK
    } catch let error as BackendError {
        return writeErrorStatus(outStatus, error: error)
    } catch let error as BackendOperationError {
        return writeOperationErrorStatus(outStatus, error: error)
    } catch {
        writeStatus(outStatus, code: MWM_STATUS_UNAVAILABLE, message: String(describing: error).ownedCString())
        return MWM_STATUS_UNAVAILABLE
    }
}

@_cdecl("mwm_backend_move_window_to_space")
public func mwm_backend_move_window_to_space(
    _ backend: UnsafeMutableRawPointer?,
    _ windowID: UInt64,
    _ spaceID: UInt64,
    _ outStatus: UnsafeMutableRawPointer?
) -> Int32 {
    verifyTransportAbiContract()

    guard let backend else {
        writeStatus(outStatus, code: MWM_STATUS_INVALID_ARGUMENT)
        return MWM_STATUS_INVALID_ARGUMENT
    }

    let handle = Unmanaged<BackendHandle>.fromOpaque(backend).takeUnretainedValue()
    do {
        try handle.moveWindowToSpace(windowID, spaceID: spaceID)
        writeStatus(outStatus, code: MWM_STATUS_OK)
        return MWM_STATUS_OK
    } catch let error as BackendError {
        return writeErrorStatus(outStatus, error: error)
    } catch let error as BackendOperationError {
        return writeOperationErrorStatus(outStatus, error: error)
    } catch {
        writeStatus(outStatus, code: MWM_STATUS_UNAVAILABLE, message: String(describing: error).ownedCString())
        return MWM_STATUS_UNAVAILABLE
    }
}

@_cdecl("mwm_backend_swap_window_frames")
public func mwm_backend_swap_window_frames(
    _ backend: UnsafeMutableRawPointer?,
    _ sourceWindowID: UInt64,
    _ sourceX: Int32,
    _ sourceY: Int32,
    _ sourceWidth: Int32,
    _ sourceHeight: Int32,
    _ targetWindowID: UInt64,
    _ targetX: Int32,
    _ targetY: Int32,
    _ targetWidth: Int32,
    _ targetHeight: Int32,
    _ outStatus: UnsafeMutableRawPointer?
) -> Int32 {
    verifyTransportAbiContract()

    guard let backend else {
        writeStatus(outStatus, code: MWM_STATUS_INVALID_ARGUMENT)
        return MWM_STATUS_INVALID_ARGUMENT
    }

    let handle = Unmanaged<BackendHandle>.fromOpaque(backend).takeUnretainedValue()
    do {
        try handle.swapWindowFrames(
            sourceWindowID: sourceWindowID,
            sourceFrame: NativeBounds(x: sourceX, y: sourceY, width: sourceWidth, height: sourceHeight),
            targetWindowID: targetWindowID,
            targetFrame: NativeBounds(x: targetX, y: targetY, width: targetWidth, height: targetHeight)
        )
        writeStatus(outStatus, code: MWM_STATUS_OK)
        return MWM_STATUS_OK
    } catch let error as BackendError {
        return writeErrorStatus(outStatus, error: error)
    } catch let error as BackendOperationError {
        return writeOperationErrorStatus(outStatus, error: error)
    } catch {
        writeStatus(outStatus, code: MWM_STATUS_UNAVAILABLE, message: String(describing: error).ownedCString())
        return MWM_STATUS_UNAVAILABLE
    }
}

@_cdecl("mwm_status_release")
public func mwm_status_release(_ status: UnsafeMutableRawPointer?) {
    verifyTransportAbiContract()

    guard let status else {
        return
    }

    status
        .assumingMemoryBound(to: MwmStatus.self)
        .pointee
        .releaseOwnedPayloads()
}

@_cdecl("mwm_desktop_snapshot_release")
public func mwm_desktop_snapshot_release(_ snapshot: UnsafeMutableRawPointer?) {
    verifyTransportAbiContract()

    guard let snapshot else {
        return
    }

    snapshot
        .assumingMemoryBound(to: MwmDesktopSnapshotAbi.self)
        .pointee
        .releaseOwnedPayloads()
}

private func operationDirection(_ direction: NativeDirection) -> String {
    switch direction {
    case .west: "west"
    case .east: "east"
    case .north: "north"
    case .south: "south"
    }
}

private extension DesktopSnapshot {
    init(snapshotABI: MwmDesktopSnapshotAbi) {
        let spaces: [DesktopSpaceSnapshot]
        if let pointer = snapshotABI.spaces_ptr {
            spaces = Array(UnsafeBufferPointer(start: pointer, count: snapshotABI.spaces_len)).map { space in
                DesktopSpaceSnapshot(
                    id: space.id,
                    displayIndex: space.display_index,
                    active: space.active != 0,
                    kind: SpaceKind(rawValue: space.kind) ?? .system
                )
            }
        } else {
            spaces = []
        }

        let windows: [DesktopWindowSnapshot]
        if let pointer = snapshotABI.windows_ptr {
            windows = Array(UnsafeBufferPointer(start: pointer, count: snapshotABI.windows_len)).map { window in
                DesktopWindowSnapshot(
                    id: window.id,
                    pid: window.has_pid == 0 ? nil : window.pid,
                    appID: window.app_id_ptr.map { String(cString: $0) },
                    title: window.title_ptr.map { String(cString: $0) },
                    bounds: window.has_frame == 0
                        ? nil
                        : NativeBounds(
                            x: window.frame.x,
                            y: window.frame.y,
                            width: window.frame.width,
                            height: window.frame.height
                        ),
                    level: window.level,
                    spaceID: window.space_id,
                    orderIndex: window.has_order_index == 0 ? nil : window.order_index
                )
            }
        } else {
            windows = []
        }

        self.init(
            spaces: spaces,
            windows: windows,
            focusedWindowID: snapshotABI.focused_window_id == 0 ? nil : snapshotABI.focused_window_id
        )
    }
}
