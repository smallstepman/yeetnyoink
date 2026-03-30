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
    private var currentActiveSpaceID: UInt64?

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
        currentActiveSpaceID = ActionRecordingSystem.activeSpaceID(in: managedDisplaySpaces.first)
    }

    func hasSymbol(_ symbol: String) -> Bool { true }
    func isAccessibilityTrusted() -> Bool { true }
    func mainConnectionID() -> UInt32? { 7 }
    func managedDisplaySpaces() throws -> [[String: Any]] {
        guard let currentActiveSpaceID else {
            return managedDisplaySpacesPayload
        }

        return managedDisplaySpacesPayload.enumerated().map { index, display in
            guard index == 0 else {
                return display
            }

            var copy = display
            if var currentSpace = copy["Current Space"] as? [String: Any] {
                currentSpace["ManagedSpaceID"] = currentActiveSpaceID
                copy["Current Space"] = currentSpace
            } else {
                copy["Current Space"] = ["ManagedSpaceID": currentActiveSpaceID]
            }
            return copy
        }
    }
    func windowsForSpace(_ spaceID: UInt64) throws -> [UInt64] { windowsBySpaceID[spaceID] ?? [] }
    func windowDescriptions(for windowIDs: [UInt64]) throws -> [SystemWindowDescription] {
        windowIDs.compactMap { windowDescriptionsByID[$0] }
    }
    func onscreenWindowOrder() throws -> [UInt64] {
        guard let currentActiveSpaceID else {
            return onscreenWindowIDs
        }
        return windowsBySpaceID[currentActiveSpaceID] ?? onscreenWindowIDs
    }
    func focusedWindowID() throws -> UInt64? {
        if let currentActiveSpaceID,
           let focusedWindowID = windowsBySpaceID[currentActiveSpaceID]?.first
        {
            return focusedWindowID
        }
        return focusedWindow
    }
    func stableAppID(for pid: UInt32) -> String? { stableAppIDsByPID[pid] }

    func switchSpace(_ spaceID: UInt64) throws {
        calls.append(.switchSpace(spaceID))
        currentActiveSpaceID = spaceID
    }

    func switchAdjacentSpace(_ direction: NativeDirection, targetSpaceID: UInt64) throws {
        calls.append(.switchAdjacentSpace(direction, targetSpaceID))
        currentActiveSpaceID = targetSpaceID
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

    private static func activeSpaceID(in display: [String: Any]?) -> UInt64? {
        guard let display else {
            return nil
        }
        if let currentSpace = display["Current Space"] as? [String: Any],
           let activeSpaceID = currentSpace["ManagedSpaceID"] as? UInt64
        {
            return activeSpaceID
        }
        return nil
    }
}

private final class SpaceSwitchSequencingSystem: BackendSystem, BackendActionSystem {
    enum Call: Equatable {
        case switchSpace(UInt64)
        case switchAdjacentSpace(NativeDirection, UInt64)
    }

    private let spaceIDs: [UInt64]
    private let windowsBySpaceID: [UInt64: [UInt64]]
    private let windowDescriptionsByID: [UInt64: SystemWindowDescription]
    private let stableAppIDsByPID: [UInt32: String]
    private let adjacentHotkeyResultSpaceID: UInt64?
    private let configuredExactSwitchActiveLagPolls: Int
    private let configuredExactSwitchOnscreenLagPolls: Int

    private(set) var calls = [Call]()
    private(set) var managedDisplaySpacesCallCount = 0
    private(set) var onscreenWindowOrderCallCount = 0

    private var activeSpaceID: UInt64
    private var pendingExactSwitchTargetSpaceID: UInt64?
    private var exactSwitchActiveLagPollsRemaining = 0
    private var exactSwitchOnscreenLagPollsRemaining = 0
    private var sourceSpaceIDBeforeExactSwitch: UInt64?

    init(
        spaceIDs: [UInt64],
        initialActiveSpaceID: UInt64,
        windowsBySpaceID: [UInt64: [UInt64]],
        windowDescriptionsByID: [UInt64: SystemWindowDescription],
        stableAppIDsByPID: [UInt32: String],
        adjacentHotkeyResultSpaceID: UInt64? = nil,
        exactSwitchActiveLagPolls: Int = 0,
        exactSwitchOnscreenLagPolls: Int = 0
    ) {
        self.spaceIDs = spaceIDs
        activeSpaceID = initialActiveSpaceID
        self.windowsBySpaceID = windowsBySpaceID
        self.windowDescriptionsByID = windowDescriptionsByID
        self.stableAppIDsByPID = stableAppIDsByPID
        self.adjacentHotkeyResultSpaceID = adjacentHotkeyResultSpaceID
        configuredExactSwitchActiveLagPolls = exactSwitchActiveLagPolls
        configuredExactSwitchOnscreenLagPolls = exactSwitchOnscreenLagPolls
    }

    func hasSymbol(_ symbol: String) -> Bool { true }
    func isAccessibilityTrusted() -> Bool { true }
    func mainConnectionID() -> UInt32? { 7 }

    func managedDisplaySpaces() throws -> [[String: Any]] {
        managedDisplaySpacesCallCount += 1
        settleActiveSpaceIfNeeded()
        return [[
            "Current Space": ["ManagedSpaceID": activeSpaceID],
            "Display Identifier": "display-1",
            "Spaces": spaceIDs.map { ["ManagedSpaceID": $0, "type": 0] },
        ]]
    }

    func windowsForSpace(_ spaceID: UInt64) throws -> [UInt64] {
        windowsBySpaceID[spaceID] ?? []
    }

    func windowDescriptions(for windowIDs: [UInt64]) throws -> [SystemWindowDescription] {
        windowIDs.compactMap { windowDescriptionsByID[$0] }
    }

    func onscreenWindowOrder() throws -> [UInt64] {
        onscreenWindowOrderCallCount += 1
        let visibleSpaceID = settleVisibleSpaceIfNeeded()
        return windowsBySpaceID[visibleSpaceID] ?? []
    }

    func focusedWindowID() throws -> UInt64? {
        (windowsBySpaceID[activeSpaceID] ?? []).first
    }

    func stableAppID(for pid: UInt32) -> String? {
        stableAppIDsByPID[pid]
    }

    func switchSpace(_ spaceID: UInt64) throws {
        calls.append(.switchSpace(spaceID))
        pendingExactSwitchTargetSpaceID = spaceID
        exactSwitchActiveLagPollsRemaining = configuredExactSwitchActiveLagPolls
        exactSwitchOnscreenLagPollsRemaining = configuredExactSwitchOnscreenLagPolls
        sourceSpaceIDBeforeExactSwitch = activeSpaceID
    }

    func switchAdjacentSpace(_ direction: NativeDirection, targetSpaceID: UInt64) throws {
        calls.append(.switchAdjacentSpace(direction, targetSpaceID))
        activeSpaceID = adjacentHotkeyResultSpaceID ?? targetSpaceID
    }

    private func settleActiveSpaceIfNeeded() {
        guard let targetSpaceID = pendingExactSwitchTargetSpaceID else {
            return
        }

        if exactSwitchActiveLagPollsRemaining > 0 {
            exactSwitchActiveLagPollsRemaining -= 1
            if exactSwitchActiveLagPollsRemaining == 0 {
                activeSpaceID = targetSpaceID
            }
            return
        }

        activeSpaceID = targetSpaceID
    }

    private func settleVisibleSpaceIfNeeded() -> UInt64 {
        guard let targetSpaceID = pendingExactSwitchTargetSpaceID else {
            return activeSpaceID
        }
        guard activeSpaceID == targetSpaceID else {
            return sourceSpaceIDBeforeExactSwitch ?? activeSpaceID
        }

        if exactSwitchOnscreenLagPollsRemaining > 0 {
            exactSwitchOnscreenLagPollsRemaining -= 1
            if exactSwitchOnscreenLagPollsRemaining == 0 {
                pendingExactSwitchTargetSpaceID = nil
                sourceSpaceIDBeforeExactSwitch = nil
                return targetSpaceID
            }
            return sourceSpaceIDBeforeExactSwitch ?? activeSpaceID
        }

        pendingExactSwitchTargetSpaceID = nil
        sourceSpaceIDBeforeExactSwitch = nil
        return targetSpaceID
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

    func testSwitchSpaceInSnapshotFallsBackToExactSwitchWhenAdjacentHotkeySkipsTargetSpace() throws {
        let system = SpaceSwitchSequencingSystem(
            spaceIDs: [1, 2, 3],
            initialActiveSpaceID: 1,
            windowsBySpaceID: [1: [10], 2: [21], 3: [31]],
            windowDescriptionsByID: [
                10: .init(
                    id: 10,
                    pid: 1010,
                    title: "source",
                    level: 0,
                    frame: .init(x: 0, y: 0, width: 100, height: 100)
                ),
            ],
            stableAppIDsByPID: [1010: "com.example.source"],
            adjacentHotkeyResultSpaceID: 3
        )
        let backend = Backend(system: system)
        let snapshot = try backend.topologySnapshot()

        try backend.switchSpaceInSnapshot(snapshot: snapshot, targetSpaceID: 2, adjacentDirection: .east)

        XCTAssertEqual(system.calls, [.switchAdjacentSpace(.east, 2), .switchSpace(2)])
    }

    func testSwitchSpaceInSnapshotWaitsForSpacePresentationBeforeReturning() throws {
        let system = SpaceSwitchSequencingSystem(
            spaceIDs: [1, 9],
            initialActiveSpaceID: 1,
            windowsBySpaceID: [1: [11], 9: [77]],
            windowDescriptionsByID: [
                11: .init(
                    id: 11,
                    pid: 1111,
                    title: "source",
                    level: 0,
                    frame: .init(x: 0, y: 0, width: 100, height: 100)
                ),
            ],
            stableAppIDsByPID: [1111: "com.example.source"],
            exactSwitchActiveLagPolls: 2,
            exactSwitchOnscreenLagPolls: 2
        )
        let backend = Backend(system: system)
        let snapshot = try backend.topologySnapshot()
        let managedDisplaySpacesCallsBeforeSwitch = system.managedDisplaySpacesCallCount
        let onscreenWindowOrderCallsBeforeSwitch = system.onscreenWindowOrderCallCount

        try backend.switchSpaceInSnapshot(snapshot: snapshot, targetSpaceID: 9, adjacentDirection: nil)

        XCTAssertEqual(system.calls, [.switchSpace(9)])
        XCTAssertGreaterThan(
            system.managedDisplaySpacesCallCount,
            managedDisplaySpacesCallsBeforeSwitch,
            "switchSpaceInSnapshot should poll active space state before returning"
        )
        XCTAssertGreaterThan(
            system.onscreenWindowOrderCallCount,
            onscreenWindowOrderCallsBeforeSwitch,
            "switchSpaceInSnapshot should poll onscreen windows before returning"
        )
    }

    func testSwitchSpaceAndRefreshReturnsSettledSnapshotAfterSwitch() throws {
        let system = SpaceSwitchSequencingSystem(
            spaceIDs: [1, 9],
            initialActiveSpaceID: 1,
            windowsBySpaceID: [1: [11], 9: [77]],
            windowDescriptionsByID: [
                11: .init(
                    id: 11,
                    pid: 1111,
                    title: "source",
                    level: 0,
                    frame: .init(x: 0, y: 0, width: 100, height: 100)
                ),
                77: .init(
                    id: 77,
                    pid: 7777,
                    title: "target",
                    level: 0,
                    frame: .init(x: 300, y: 0, width: 100, height: 100)
                ),
            ],
            stableAppIDsByPID: [
                1111: "com.example.source",
                7777: "com.example.target",
            ],
            exactSwitchActiveLagPolls: 2,
            exactSwitchOnscreenLagPolls: 2
        )
        let backend = Backend(system: system)
        let snapshot = try backend.topologySnapshot()
        let managedDisplaySpacesCallsBeforeSwitch = system.managedDisplaySpacesCallCount
        let onscreenWindowOrderCallsBeforeSwitch = system.onscreenWindowOrderCallCount

        let refreshed = try backend.switchSpaceAndRefresh(
            snapshot: snapshot,
            targetSpaceID: 9,
            adjacentDirection: nil
        )

        XCTAssertEqual(system.calls, [.switchSpace(9)])
        XCTAssertEqual(
            Set(refreshed.spaces.filter(\.active).map(\.id)),
            Set([9 as UInt64])
        )
        XCTAssertEqual(
            refreshed.windows.first(where: { $0.id == 77 })?.orderIndex,
            0,
            "refreshed snapshot should expose the target space window as visible after settling"
        )
        XCTAssertNil(
            refreshed.windows.first(where: { $0.id == 11 })?.orderIndex,
            "refreshed snapshot should demote the source-space window to an inactive placeholder"
        )
        XCTAssertEqual(refreshed.focusedWindowID, 77)
        XCTAssertGreaterThan(
            system.managedDisplaySpacesCallCount,
            managedDisplaySpacesCallsBeforeSwitch,
            "switchSpaceAndRefresh should poll active space state before returning"
        )
        XCTAssertGreaterThan(
            system.onscreenWindowOrderCallCount,
            onscreenWindowOrderCallsBeforeSwitch,
            "switchSpaceAndRefresh should poll onscreen windows before returning"
        )
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
