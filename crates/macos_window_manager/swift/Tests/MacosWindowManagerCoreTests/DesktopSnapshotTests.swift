import MacosWindowManagerCore
import XCTest

private enum TestFocusedWindowError: Error {
    case probeFailed
}

private struct FocusedWindowFailingSystem: BackendSystem {
    let requiredSymbols: [String]
    let accessibilityTrusted: Bool
    let mainConnectionIDValue: UInt32?
    let managedDisplaySpacesPayload: [[String: Any]]
    let windowsBySpaceID: [UInt64: [UInt64]]
    let windowDescriptionsByID: [UInt64: SystemWindowDescription]
    let onscreenWindowIDs: [UInt64]
    let stableAppIDsByPID: [UInt32: String]

    func hasSymbol(_ symbol: String) -> Bool {
        requiredSymbols.contains(symbol)
    }

    func isAccessibilityTrusted() -> Bool {
        accessibilityTrusted
    }

    func mainConnectionID() -> UInt32? {
        mainConnectionIDValue
    }

    func managedDisplaySpaces() throws -> [[String: Any]] {
        managedDisplaySpacesPayload
    }

    func windowsForSpace(_ spaceID: UInt64) throws -> [UInt64] {
        windowsBySpaceID[spaceID] ?? []
    }

    func windowDescriptions(for windowIDs: [UInt64]) throws -> [SystemWindowDescription] {
        windowIDs.compactMap { windowDescriptionsByID[$0] }
    }

    func onscreenWindowOrder() throws -> [UInt64] {
        onscreenWindowIDs
    }

    func focusedWindowID() throws -> UInt64? {
        throw TestFocusedWindowError.probeFailed
    }

    func stableAppID(for pid: UInt32) -> String? {
        stableAppIDsByPID[pid]
    }
}

final class DesktopSnapshotTests: XCTestCase {
    func testDesktopSnapshotFlattensSpacesAndWindows() throws {
        let backend = Backend(system: FakeSystem(
            requiredSymbols: PrivateSymbols.requiredSymbols,
            accessibilityTrusted: true,
            mainConnectionID: 7,
            managedDisplaySpaces: [
                [
                    "Current Space": [
                        "ManagedSpaceID": 101 as UInt64,
                    ],
                    "Display Identifier": "display-1",
                    "Spaces": [
                        [
                            "ManagedSpaceID": 101 as UInt64,
                            "type": 0,
                        ],
                        [
                            "ManagedSpaceID": 102 as UInt64,
                            "type": 0,
                            "TileLayoutManager": [
                                "TileSpaces": [
                                    ["ManagedSpaceID": 201 as UInt64],
                                    ["id64": 202 as UInt64],
                                ],
                            ],
                        ],
                    ],
                ],
            ],
            windowsBySpaceID: [
                101: [9001, 9002],
                102: [9003],
            ],
            windowDescriptionsByID: [
                9001: .init(
                    id: 9001,
                    pid: 41,
                    title: "Terminal",
                    level: 0,
                    frame: .init(x: 0, y: 0, width: 900, height: 700)
                ),
                9002: .init(
                    id: 9002,
                    pid: 42,
                    title: "Safari",
                    level: 3,
                    frame: .init(x: 20, y: 10, width: 600, height: 500)
                ),
                9003: .init(
                    id: 9003,
                    pid: 43,
                    title: "Notes",
                    level: 0,
                    frame: .init(x: 100, y: 50, width: 400, height: 300)
                ),
            ],
            onscreenWindowOrder: [9003, 9001, 9002],
            focusedWindowID: 9003,
            stableAppIDsByPID: [
                41: "com.apple.Terminal",
                42: "com.apple.Safari",
                43: "com.apple.Notes",
            ]
        ))

        let snapshot = try backend.desktopSnapshot()

        XCTAssertEqual(snapshot.spaces.count, 2)
        XCTAssertEqual(snapshot.windows.count, 3)
        XCTAssertEqual(snapshot.spaces.map(\.id), [101, 102])
        XCTAssertEqual(snapshot.windows.map(\.id), [9001, 9002, 9003])
        XCTAssertEqual(snapshot.focusedWindowID, 9003)
        XCTAssertEqual(snapshot.windows.first?.appID, "com.apple.Terminal")
        XCTAssertEqual(snapshot.windows.last?.spaceID, 102)
    }

    func testDesktopSnapshotReturnsTopologyWhenFocusedWindowProbeFails() throws {
        let backend = Backend(system: FocusedWindowFailingSystem(
            requiredSymbols: PrivateSymbols.requiredSymbols,
            accessibilityTrusted: true,
            mainConnectionIDValue: 7,
            managedDisplaySpacesPayload: [
                [
                    "Current Space": [
                        "ManagedSpaceID": 101 as UInt64,
                    ],
                    "Display Identifier": "display-1",
                    "Spaces": [
                        [
                            "ManagedSpaceID": 101 as UInt64,
                            "type": 0,
                        ],
                        [
                            "ManagedSpaceID": 102 as UInt64,
                            "type": 0,
                        ],
                    ],
                ],
            ],
            windowsBySpaceID: [
                101: [9001, 9002],
                102: [9003],
            ],
            windowDescriptionsByID: [
                9001: .init(
                    id: 9001,
                    pid: 41,
                    title: "Terminal",
                    level: 0,
                    frame: .init(x: 0, y: 0, width: 900, height: 700)
                ),
                9002: .init(
                    id: 9002,
                    pid: 42,
                    title: "Safari",
                    level: 3,
                    frame: .init(x: 20, y: 10, width: 600, height: 500)
                ),
                9003: .init(
                    id: 9003,
                    pid: 43,
                    title: "Notes",
                    level: 0,
                    frame: .init(x: 100, y: 50, width: 400, height: 300)
                ),
            ],
            onscreenWindowIDs: [9003, 9001, 9002],
            stableAppIDsByPID: [
                41: "com.apple.Terminal",
                42: "com.apple.Safari",
                43: "com.apple.Notes",
            ]
        ))

        let snapshot = try backend.desktopSnapshot()

        XCTAssertEqual(snapshot.spaces.map(\.id), [101, 102])
        XCTAssertEqual(snapshot.windows.map(\.id), [9001, 9002, 9003])
        XCTAssertNil(snapshot.focusedWindowID)
    }
}
