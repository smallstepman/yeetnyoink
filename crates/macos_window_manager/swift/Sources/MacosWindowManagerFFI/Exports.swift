import MacosWindowManagerCore

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
