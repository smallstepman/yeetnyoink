import MacosWindowManagerCore
import XCTest

final class EnvironmentTests: XCTestCase {
    func testValidateEnvironmentReportsMissingAccessibilityPermission() {
        let backend = Backend(system: FakeSystem(
            requiredSymbols: PrivateSymbols.requiredSymbols,
            accessibilityTrusted: false,
            mainConnectionID: 7,
            managedDisplaySpaces: [],
            windowsBySpaceID: [:],
            windowDescriptionsByID: [:],
            onscreenWindowOrder: [],
            focusedWindowID: nil,
            stableAppIDsByPID: [:]
        ))

        XCTAssertThrowsError(try backend.validateEnvironment()) { error in
            XCTAssertEqual(error as? BackendError, .missingAccessibilityPermission)
        }
    }

    func testPrepareFastFocusContextReturnsValidatedEnvironmentAndSnapshot() throws {
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

        let context = try backend.prepareFastFocusContext()

        XCTAssertEqual(context.environment, .validated)
        XCTAssertEqual(context.desktopSnapshot.focusedWindowID, 9003)
        XCTAssertEqual(context.desktopSnapshot.windows.map(\.id), [9001, 9002, 9003])
    }
}
