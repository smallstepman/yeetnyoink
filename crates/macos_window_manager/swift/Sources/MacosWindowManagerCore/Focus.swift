import Foundation

public enum NativeDirection: Int32, Equatable {
    case west = 0
    case east = 1
    case north = 2
    case south = 3
}

public struct ActiveSpaceFocusTargetHint: Equatable {
    public let spaceID: UInt64
    public let bounds: NativeBounds

    public init(spaceID: UInt64, bounds: NativeBounds) {
        self.spaceID = spaceID
        self.bounds = bounds
    }
}

public protocol BackendActionSystem: BackendSystem {
    func switchSpace(_ spaceID: UInt64) throws
    func switchAdjacentSpace(_ direction: NativeDirection, targetSpaceID: UInt64) throws
    func focusWindow(_ windowID: UInt64) throws
    func focusWindowWithKnownPID(_ windowID: UInt64, pid: UInt32) throws
    func axWindowIDs(for pid: UInt32) throws -> [UInt64]
    func moveWindowToSpace(_ windowID: UInt64, spaceID: UInt64) throws
    func swapWindowFrames(
        sourceWindowID: UInt64,
        sourceFrame: NativeBounds,
        targetWindowID: UInt64,
        targetFrame: NativeBounds
    ) throws
}

public extension BackendActionSystem {
    func switchSpace(_ spaceID: UInt64) throws {
        throw BackendOperationError.callFailed("switch_space")
    }

    func switchAdjacentSpace(_ direction: NativeDirection, targetSpaceID: UInt64) throws {
        try switchSpace(targetSpaceID)
    }

    func focusWindow(_ windowID: UInt64) throws {
        throw BackendOperationError.callFailed("focus_window")
    }

    func focusWindowWithKnownPID(_ windowID: UInt64, pid: UInt32) throws {
        try focusWindow(windowID)
    }

    func axWindowIDs(for pid: UInt32) throws -> [UInt64] {
        []
    }

    func moveWindowToSpace(_ windowID: UInt64, spaceID: UInt64) throws {
        throw BackendOperationError.callFailed("move_window_to_space")
    }

    func swapWindowFrames(
        sourceWindowID: UInt64,
        sourceFrame: NativeBounds,
        targetWindowID: UInt64,
        targetFrame: NativeBounds
    ) throws {
        throw BackendOperationError.callFailed("swap_window_frames")
    }
}
