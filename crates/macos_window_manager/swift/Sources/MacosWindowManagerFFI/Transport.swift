import MacosWindowManagerCore

public let MWM_STATUS_OK: Int32 = 0
public let MWM_STATUS_INVALID_ARGUMENT: Int32 = 1
public let MWM_STATUS_UNAVAILABLE: Int32 = 2
public let MWM_STATUS_CONNECT_MISSING_REQUIRED_SYMBOL: Int32 = 10
public let MWM_STATUS_CONNECT_MISSING_ACCESSIBILITY_PERMISSION: Int32 = 11
public let MWM_STATUS_CONNECT_MISSING_TOPOLOGY_PRECONDITION: Int32 = 12
public let MWM_STATUS_PROBE_MISSING_TOPOLOGY: Int32 = 20

/// Runtime ABI guards for the transport shared with Rust `src/transport.rs`.
///
/// Pointer payloads are owned by the Swift FFI layer when non-nil and must be
/// released by Rust via `mwm_status_release` or `mwm_desktop_snapshot_release`
/// after copying any needed values out of the transport structs.
@inline(__always)
func verifyTransportAbiContract() {
    precondition(MemoryLayout<MwmStatus>.stride == 16)
    precondition(MemoryLayout<MwmStatus>.alignment == 8)
    precondition(MemoryLayout<MwmStatus>.offset(of: \.code) == 0)
    precondition(MemoryLayout<MwmStatus>.offset(of: \.message_ptr) == 8)

    precondition(MemoryLayout<MwmRectAbi>.stride == 16)
    precondition(MemoryLayout<MwmRectAbi>.alignment == 4)
    precondition(MemoryLayout<MwmRectAbi>.offset(of: \.x) == 0)
    precondition(MemoryLayout<MwmRectAbi>.offset(of: \.y) == 4)
    precondition(MemoryLayout<MwmRectAbi>.offset(of: \.width) == 8)
    precondition(MemoryLayout<MwmRectAbi>.offset(of: \.height) == 12)

    precondition(MemoryLayout<MwmSpaceAbi>.stride == 24)
    precondition(MemoryLayout<MwmSpaceAbi>.alignment == 8)
    precondition(MemoryLayout<MwmSpaceAbi>.offset(of: \.id) == 0)
    precondition(MemoryLayout<MwmSpaceAbi>.offset(of: \.display_index) == 8)
    precondition(MemoryLayout<MwmSpaceAbi>.offset(of: \.active) == 16)
    precondition(MemoryLayout<MwmSpaceAbi>.offset(of: \.kind) == 20)

    precondition(MemoryLayout<MwmWindowAbi>.stride == 80)
    precondition(MemoryLayout<MwmWindowAbi>.alignment == 8)
    precondition(MemoryLayout<MwmWindowAbi>.offset(of: \.id) == 0)
    precondition(MemoryLayout<MwmWindowAbi>.offset(of: \.pid) == 8)
    precondition(MemoryLayout<MwmWindowAbi>.offset(of: \.has_pid) == 12)
    precondition(MemoryLayout<MwmWindowAbi>.offset(of: \.app_id_ptr) == 16)
    precondition(MemoryLayout<MwmWindowAbi>.offset(of: \.title_ptr) == 24)
    precondition(MemoryLayout<MwmWindowAbi>.offset(of: \.frame) == 32)
    precondition(MemoryLayout<MwmWindowAbi>.offset(of: \.has_frame) == 48)
    precondition(MemoryLayout<MwmWindowAbi>.offset(of: \.level) == 52)
    precondition(MemoryLayout<MwmWindowAbi>.offset(of: \.space_id) == 56)
    precondition(MemoryLayout<MwmWindowAbi>.offset(of: \.order_index) == 64)
    precondition(MemoryLayout<MwmWindowAbi>.offset(of: \.has_order_index) == 72)

    precondition(MemoryLayout<MwmDesktopSnapshotAbi>.stride == 40)
    precondition(MemoryLayout<MwmDesktopSnapshotAbi>.alignment == 8)
    precondition(MemoryLayout<MwmDesktopSnapshotAbi>.offset(of: \.spaces_ptr) == 0)
    precondition(MemoryLayout<MwmDesktopSnapshotAbi>.offset(of: \.spaces_len) == 8)
    precondition(MemoryLayout<MwmDesktopSnapshotAbi>.offset(of: \.windows_ptr) == 16)
    precondition(MemoryLayout<MwmDesktopSnapshotAbi>.offset(of: \.windows_len) == 24)
    precondition(MemoryLayout<MwmDesktopSnapshotAbi>.offset(of: \.focused_window_id) == 32)
}

/// FFI status transport shared with Rust.
///
/// `message_ptr` is owned by the Swift FFI layer when non-nil. Rust must copy
/// the string and then call `mwm_status_release` to release the owned payload.
public struct MwmStatus {
    public var code: Int32
    public var message_ptr: UnsafeMutablePointer<CChar>?

    public init(
        code: Int32 = MWM_STATUS_OK,
        message_ptr: UnsafeMutablePointer<CChar>? = nil
    ) {
        self.code = code
        self.message_ptr = message_ptr
    }
}

/// FFI rectangle transport shared with Rust.
public struct MwmRectAbi {
    public var x: Int32
    public var y: Int32
    public var width: Int32
    public var height: Int32

    public init(x: Int32 = 0, y: Int32 = 0, width: Int32 = 0, height: Int32 = 0) {
        self.x = x
        self.y = y
        self.width = width
        self.height = height
    }
}

/// FFI space transport shared with Rust.
public struct MwmSpaceAbi {
    public var id: UInt64
    public var display_index: Int
    public var active: UInt8
    public var kind: Int32

    public init(
        id: UInt64 = 0,
        display_index: Int = 0,
        active: UInt8 = 0,
        kind: Int32 = 0
    ) {
        self.id = id
        self.display_index = display_index
        self.active = active
        self.kind = kind
    }
}

/// FFI window transport shared with Rust.
///
/// `app_id_ptr` and `title_ptr` are owned by the Swift FFI layer when non-nil
/// and are released as part of `mwm_desktop_snapshot_release`.
public struct MwmWindowAbi {
    public var id: UInt64
    public var pid: UInt32
    public var has_pid: UInt8
    public var app_id_ptr: UnsafeMutablePointer<CChar>?
    public var title_ptr: UnsafeMutablePointer<CChar>?
    public var frame: MwmRectAbi
    public var has_frame: UInt8
    public var level: Int32
    public var space_id: UInt64
    public var order_index: Int
    public var has_order_index: UInt8

    public init(
        id: UInt64 = 0,
        pid: UInt32 = 0,
        has_pid: UInt8 = 0,
        app_id_ptr: UnsafeMutablePointer<CChar>? = nil,
        title_ptr: UnsafeMutablePointer<CChar>? = nil,
        frame: MwmRectAbi = MwmRectAbi(),
        has_frame: UInt8 = 0,
        level: Int32 = 0,
        space_id: UInt64 = 0,
        order_index: Int = 0,
        has_order_index: UInt8 = 0
    ) {
        self.id = id
        self.pid = pid
        self.has_pid = has_pid
        self.app_id_ptr = app_id_ptr
        self.title_ptr = title_ptr
        self.frame = frame
        self.has_frame = has_frame
        self.level = level
        self.space_id = space_id
        self.order_index = order_index
        self.has_order_index = has_order_index
    }
}

/// FFI desktop snapshot transport shared with Rust.
///
/// Any non-nil pointer fields are owned by the Swift FFI layer and must be
/// released by Rust via `mwm_desktop_snapshot_release` after copying the
/// snapshot contents into Rust-owned structures.
public struct MwmDesktopSnapshotAbi {
    public var spaces_ptr: UnsafeMutablePointer<MwmSpaceAbi>?
    public var spaces_len: Int
    public var windows_ptr: UnsafeMutablePointer<MwmWindowAbi>?
    public var windows_len: Int
    public var focused_window_id: UInt64

    public init(
        spaces_ptr: UnsafeMutablePointer<MwmSpaceAbi>? = nil,
        spaces_len: Int = 0,
        windows_ptr: UnsafeMutablePointer<MwmWindowAbi>? = nil,
        windows_len: Int = 0,
        focused_window_id: UInt64 = 0
    ) {
        self.spaces_ptr = spaces_ptr
        self.spaces_len = spaces_len
        self.windows_ptr = windows_ptr
        self.windows_len = windows_len
        self.focused_window_id = focused_window_id
    }
}

extension MwmStatus {
    mutating func releaseOwnedPayloads() {
        message_ptr?.deallocate()
        self = MwmStatus()
    }
}

extension MwmWindowAbi {
    mutating func releaseOwnedPayloads() {
        app_id_ptr?.deallocate()
        title_ptr?.deallocate()
        self = MwmWindowAbi(
            id: id,
            pid: pid,
            has_pid: has_pid,
            app_id_ptr: nil,
            title_ptr: nil,
            frame: frame,
            has_frame: has_frame,
            level: level,
            space_id: space_id,
            order_index: order_index,
            has_order_index: has_order_index
        )
    }
}

extension MwmDesktopSnapshotAbi {
    mutating func releaseOwnedPayloads() {
        if let windows_ptr {
            for index in 0..<windows_len {
                windows_ptr.advanced(by: index).pointee.releaseOwnedPayloads()
            }
            windows_ptr.deallocate()
        }

        spaces_ptr?.deallocate()
        self = MwmDesktopSnapshotAbi()
    }
}

extension MwmRectAbi {
    init(_ bounds: NativeBounds) {
        self.init(
            x: bounds.x,
            y: bounds.y,
            width: bounds.width,
            height: bounds.height
        )
    }
}

extension MwmSpaceAbi {
    init(_ space: DesktopSpaceSnapshot) {
        self.init(
            id: space.id,
            display_index: space.displayIndex,
            active: space.active ? 1 : 0,
            kind: space.kind.rawValue
        )
    }
}

extension MwmWindowAbi {
    init(_ window: DesktopWindowSnapshot) {
        self.init(
            id: window.id,
            pid: window.pid ?? 0,
            has_pid: window.pid == nil ? 0 : 1,
            app_id_ptr: window.appID?.ownedCString(),
            title_ptr: window.title?.ownedCString(),
            frame: window.bounds.map(MwmRectAbi.init) ?? MwmRectAbi(),
            has_frame: window.bounds == nil ? 0 : 1,
            level: window.level,
            space_id: window.spaceID,
            order_index: window.orderIndex ?? 0,
            has_order_index: window.orderIndex == nil ? 0 : 1
        )
    }
}

extension MwmDesktopSnapshotAbi {
    init(_ snapshot: DesktopSnapshot) {
        let spacesPointer: UnsafeMutablePointer<MwmSpaceAbi>?
        if snapshot.spaces.isEmpty {
            spacesPointer = nil
        } else {
            let pointer = UnsafeMutablePointer<MwmSpaceAbi>.allocate(capacity: snapshot.spaces.count)
            pointer.initialize(from: snapshot.spaces.map(MwmSpaceAbi.init), count: snapshot.spaces.count)
            spacesPointer = pointer
        }

        let windowsPointer: UnsafeMutablePointer<MwmWindowAbi>?
        if snapshot.windows.isEmpty {
            windowsPointer = nil
        } else {
            let pointer = UnsafeMutablePointer<MwmWindowAbi>.allocate(capacity: snapshot.windows.count)
            pointer.initialize(from: snapshot.windows.map(MwmWindowAbi.init), count: snapshot.windows.count)
            windowsPointer = pointer
        }

        self.init(
            spaces_ptr: spacesPointer,
            spaces_len: snapshot.spaces.count,
            windows_ptr: windowsPointer,
            windows_len: snapshot.windows.count,
            focused_window_id: snapshot.focusedWindowID ?? 0
        )
    }
}

extension String {
    func ownedCString() -> UnsafeMutablePointer<CChar> {
        let buffer = UnsafeMutablePointer<CChar>.allocate(capacity: utf8.count + 1)
        utf8CString.withUnsafeBufferPointer { value in
            buffer.initialize(from: value.baseAddress!, count: value.count)
        }
        return buffer
    }
}
