public let MWM_STATUS_OK: Int32 = 0
public let MWM_STATUS_INVALID_ARGUMENT: Int32 = 1
public let MWM_STATUS_UNAVAILABLE: Int32 = 2

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
