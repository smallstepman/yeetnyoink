import MacosWindowManagerCore

private final class BackendHandle {
    func desktopSnapshot() -> MwmDesktopSnapshotAbi {
        MwmDesktopSnapshotAbi()
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
    outSnapshot
        .assumingMemoryBound(to: MwmDesktopSnapshotAbi.self)
        .pointee = handle.desktopSnapshot()
    writeStatus(outStatus, code: MWM_STATUS_OK)
    return MWM_STATUS_OK
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
