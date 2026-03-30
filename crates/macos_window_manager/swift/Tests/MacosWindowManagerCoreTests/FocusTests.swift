import MacosWindowManagerCore
import XCTest

private final class ActionRecordingSystem: BackendSystem, BackendActionSystem {
    enum Call: Equatable {
        case switchSpace(UInt64)
        case switchAdjacentSpace(NativeDirection, UInt64)
        case focusWindow(UInt64)
        case focusWindowWithKnownPID(UInt64, UInt32)
    }

    let managedDisplaySpacesPayload: [[String: Any]]
    let windowsBySpaceID: [UInt64: [UInt64]]
    let windowDescriptionsByID: [UInt64: SystemWindowDescription]
    let onscreenWindowIDs: [UInt64]
    let focusedWindow: UInt64?
    let stableAppIDsByPID: [UInt32: String]
    let axBackedWindowIDs: [UInt32: [UInt64]]
    let focusableWindowIDs: Set<UInt64>
    private(set) var calls = [Call]()

    init(
        managedDisplaySpaces: [[String: Any]],
        windowsBySpaceID: [UInt64: [UInt64]],
        windowDescriptionsByID: [UInt64: SystemWindowDescription],
        onscreenWindowOrder: [UInt64],
        focusedWindowID: UInt64?,
        stableAppIDsByPID: [UInt32: String],
        axBackedWindowIDs: [UInt32: [UInt64]] = [:],
        focusableWindowIDs: Set<UInt64> = []
    ) {
        managedDisplaySpacesPayload = managedDisplaySpaces
        self.windowsBySpaceID = windowsBySpaceID
        self.windowDescriptionsByID = windowDescriptionsByID
        onscreenWindowIDs = onscreenWindowOrder
        focusedWindow = focusedWindowID
        self.stableAppIDsByPID = stableAppIDsByPID
        self.axBackedWindowIDs = axBackedWindowIDs
        self.focusableWindowIDs = focusableWindowIDs
    }

    func hasSymbol(_ symbol: String) -> Bool { true }
    func isAccessibilityTrusted() -> Bool { true }
    func mainConnectionID() -> UInt32? { 7 }
    func managedDisplaySpaces() throws -> [[String: Any]] { managedDisplaySpacesPayload }
    func windowsForSpace(_ spaceID: UInt64) throws -> [UInt64] { windowsBySpaceID[spaceID] ?? [] }
    func windowDescriptions(for windowIDs: [UInt64]) throws -> [SystemWindowDescription] {
        windowIDs.compactMap { windowDescriptionsByID[$0] }
    }
    func onscreenWindowOrder() throws -> [UInt64] { onscreenWindowIDs }
    func focusedWindowID() throws -> UInt64? { focusedWindow }
    func stableAppID(for pid: UInt32) -> String? { stableAppIDsByPID[pid] }

    func switchSpace(_ spaceID: UInt64) throws {
        calls.append(.switchSpace(spaceID))
    }

    func switchAdjacentSpace(_ direction: NativeDirection, targetSpaceID: UInt64) throws {
        calls.append(.switchAdjacentSpace(direction, targetSpaceID))
    }

    func focusWindow(_ windowID: UInt64) throws {
        calls.append(.focusWindow(windowID))
    }

    func focusWindowWithKnownPID(_ windowID: UInt64, pid: UInt32) throws {
        calls.append(.focusWindowWithKnownPID(windowID, pid))
        guard focusableWindowIDs.contains(windowID) else {
            throw BackendOperationError.missingWindow(windowID)
        }
    }

    func axWindowIDs(for pid: UInt32) throws -> [UInt64] {
        axBackedWindowIDs[pid] ?? []
    }
}

final class FocusTests: XCTestCase {
    func testSwitchSpaceInSnapshotUsesAdjacentDirectionWhenAvailable() throws {
        let system = ActionRecordingSystem(
            managedDisplaySpaces: [
                [
                    "Current Space": ["ManagedSpaceID": 1 as UInt64],
                    "Display Identifier": "display-1",
                    "Spaces": [
                        ["ManagedSpaceID": 1 as UInt64, "type": 0],
                        [
                            "ManagedSpaceID": 7 as UInt64,
                            "type": 0,
                            "TileLayoutManager": ["TileSpaces": [["ManagedSpaceID": 70 as UInt64]]],
                        ],
                    ],
                ],
            ],
            windowsBySpaceID: [1: [11], 7: [71]],
            windowDescriptionsByID: [
                11: .init(id: 11, pid: 11, title: "source", level: 0, frame: .init(x: 300, y: 0, width: 100, height: 100)),
            ],
            onscreenWindowOrder: [11],
            focusedWindowID: 11,
            stableAppIDsByPID: [11: "com.example.source"]
        )
        let backend = Backend(system: system)
        let snapshot = try backend.topologySnapshot()

        try backend.switchSpaceInSnapshot(snapshot: snapshot, targetSpaceID: 7, adjacentDirection: .west)

        XCTAssertEqual(system.calls, [.switchAdjacentSpace(.west, 7)])
    }

    func testFocusWindowInActiveSpaceWithKnownPidRemapsSamePidTarget() throws {
        let system = ActionRecordingSystem(
            managedDisplaySpaces: [
                [
                    "Current Space": ["ManagedSpaceID": 2 as UInt64],
                    "Display Identifier": "display-1",
                    "Spaces": [
                        [
                            "ManagedSpaceID": 2 as UInt64,
                            "type": 0,
                            "TileLayoutManager": ["TileSpaces": [["ManagedSpaceID": 201 as UInt64]]],
                        ],
                    ],
                ],
            ],
            windowsBySpaceID: [2: [985, 410]],
            windowDescriptionsByID: [
                985: .init(
                    id: 985,
                    pid: 4613,
                    appID: "com.github.wez.wezterm",
                    title: "actual-left",
                    level: 0,
                    visibleIndex: 0,
                    frame: .init(x: 12, y: 0, width: 108, height: 120)
                ),
                410: .init(
                    id: 410,
                    pid: 4613,
                    appID: "com.github.wez.wezterm",
                    title: "actual-right",
                    level: 0,
                    visibleIndex: 1,
                    frame: .init(x: 228, y: 0, width: 112, height: 120)
                ),
            ],
            onscreenWindowOrder: [985, 410],
            focusedWindowID: 985,
            stableAppIDsByPID: [4613: "com.github.wez.wezterm"],
            axBackedWindowIDs: [4613: [985, 410]],
            focusableWindowIDs: [985, 410]
        )
        let backend = Backend(system: system)

        try backend.focusWindowInActiveSpaceWithKnownPID(
            1019,
            pid: 4613,
            targetHint: ActiveSpaceFocusTargetHint(
                spaceID: 2,
                bounds: .init(x: 220, y: 0, width: 121, height: 120)
            )
        )

        XCTAssertEqual(
            system.calls,
            [
                .focusWindowWithKnownPID(1019, 4613),
                .focusWindowWithKnownPID(410, 4613),
            ]
        )
    }
}
