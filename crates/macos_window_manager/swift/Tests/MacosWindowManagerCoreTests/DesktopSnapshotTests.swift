import MacosWindowManagerCore
import XCTest

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
}
