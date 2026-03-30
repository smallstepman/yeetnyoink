import Foundation

public extension Backend {
    func switchSpace(_ spaceID: UInt64) throws {
        try actionSystem().switchSpace(spaceID)
    }

    func switchAdjacentSpace(_ direction: NativeDirection, targetSpaceID: UInt64) throws {
        try actionSystem().switchAdjacentSpace(direction, targetSpaceID: targetSpaceID)
    }

    func switchSpaceInSnapshot(
        snapshot: DesktopSnapshot,
        targetSpaceID: UInt64,
        adjacentDirection: NativeDirection?
    ) throws {
        guard let targetSpace = snapshot.space(id: targetSpaceID) else {
            throw BackendOperationError.missingSpace(targetSpaceID)
        }
        if targetSpace.kind == SpaceKind.stageManagerOpaque {
            throw BackendOperationError.unsupportedStageManagerSpace(targetSpaceID)
        }
        if snapshot.activeSpaceIDs.contains(targetSpaceID) {
            return
        }

        let targetWindowIDs = Set(snapshot.windows.lazy.filter { $0.spaceID == targetSpaceID }.map(\.id))
        if let adjacentDirection, !targetWindowIDs.isEmpty {
            try switchAdjacentSpace(adjacentDirection, targetSpaceID: targetSpaceID)
            return
        }

        try switchSpace(targetSpaceID)
    }
}
