import MacosWindowManagerCore
import XCTest

private final class FastFocusRecordingSystem: BackendSystem {
    let requiredSymbols: [String]
    let accessibilityTrusted: Bool
    let mainConnectionIDValue: UInt32?
    let managedDisplaySpacesPayload: [[String: Any]]
    let windowsBySpaceID: [UInt64: [UInt64]]
    let windowDescriptionsByID: [UInt64: SystemWindowDescription]
    let onscreenWindowIDs: [UInt64]
    let focusedWindow: UInt64?
    let stableAppIDsByPID: [UInt32: String]
    let focusedWindowDetailsError: Error?
    let focusedWindowIDError: Error?

    private(set) var managedDisplaySpacesCallCount = 0
    private(set) var mainConnectionIDCallCount = 0
    private(set) var windowsForSpaceCalls = [UInt64]()
    private(set) var windowDescriptionRequests = [[UInt64]]()
    private(set) var focusedWindowDetailsCallCount = 0

    init(
        requiredSymbols: [String],
        accessibilityTrusted: Bool,
        mainConnectionID: UInt32?,
        managedDisplaySpaces: [[String: Any]],
        windowsBySpaceID: [UInt64: [UInt64]],
        windowDescriptionsByID: [UInt64: SystemWindowDescription],
        onscreenWindowOrder: [UInt64],
        focusedWindowID: UInt64?,
        stableAppIDsByPID: [UInt32: String],
        focusedWindowDetailsError: Error? = nil,
        focusedWindowIDError: Error? = nil
    ) {
        self.requiredSymbols = requiredSymbols
        self.accessibilityTrusted = accessibilityTrusted
        mainConnectionIDValue = mainConnectionID
        managedDisplaySpacesPayload = managedDisplaySpaces
        self.windowsBySpaceID = windowsBySpaceID
        self.windowDescriptionsByID = windowDescriptionsByID
        onscreenWindowIDs = onscreenWindowOrder
        focusedWindow = focusedWindowID
        self.stableAppIDsByPID = stableAppIDsByPID
        self.focusedWindowDetailsError = focusedWindowDetailsError
        self.focusedWindowIDError = focusedWindowIDError
    }

    func hasSymbol(_ symbol: String) -> Bool {
        requiredSymbols.contains(symbol)
    }

    func isAccessibilityTrusted() -> Bool {
        accessibilityTrusted
    }

    func mainConnectionID() -> UInt32? {
        mainConnectionIDCallCount += 1
        return mainConnectionIDValue
    }

    func managedDisplaySpaces() throws -> [[String: Any]] {
        managedDisplaySpacesCallCount += 1
        return managedDisplaySpacesPayload
    }

    func windowsForSpace(_ spaceID: UInt64) throws -> [UInt64] {
        windowsForSpaceCalls.append(spaceID)
        return windowsBySpaceID[spaceID] ?? []
    }

    func windowDescriptions(for windowIDs: [UInt64]) throws -> [SystemWindowDescription] {
        windowDescriptionRequests.append(windowIDs)
        return windowIDs.compactMap { windowDescriptionsByID[$0] }
    }

    func onscreenWindowOrder() throws -> [UInt64] {
        onscreenWindowIDs
    }

    func focusedWindowID() throws -> UInt64? {
        if let focusedWindowIDError {
            throw focusedWindowIDError
        }
        return focusedWindow
    }

    func stableAppID(for pid: UInt32) -> String? {
        stableAppIDsByPID[pid]
    }

    func focusedWindowDetails() throws -> FocusedWindowDetails? {
        focusedWindowDetailsCallCount += 1
        if let focusedWindowDetailsError {
            throw focusedWindowDetailsError
        }
        guard let focusedWindow,
              let window = windowDescriptionsByID[focusedWindow],
              let pid = window.pid
        else {
            return nil
        }

        return FocusedWindowDetails(
            pid: pid,
            appID: window.appID ?? stableAppIDsByPID[pid],
            title: window.title
        )
    }
}

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

    func testPrepareFastFocusContextReturnsValidatedEnvironmentAndSyntheticFocusedWindowSnapshot() throws {
        let backend = Backend(system: FakeSystem(
            requiredSymbols: PrivateSymbols.fastFocusRequiredSymbols,
            accessibilityTrusted: true,
            mainConnectionID: nil,
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
        XCTAssertEqual(context.desktopSnapshot.focusedWindowID, 1)
        XCTAssertEqual(context.desktopSnapshot.windows.count, 1)
        XCTAssertEqual(context.desktopSnapshot.windows.first?.pid, 43)
        XCTAssertEqual(context.desktopSnapshot.windows.first?.appID, "com.apple.Notes")
        XCTAssertEqual(context.desktopSnapshot.windows.first?.title, "Notes")
    }

    func testPrepareFastFocusContextDoesNotRequireSkyLightConnectionSymbols() throws {
        let backend = Backend(system: FakeSystem(
            requiredSymbols: [
                "AXIsProcessTrusted",
            ],
            accessibilityTrusted: true,
            mainConnectionID: nil,
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
                    ],
                ],
            ],
            windowsBySpaceID: [
                101: [9001],
            ],
            windowDescriptionsByID: [
                9001: .init(
                    id: 9001,
                    pid: 41,
                    title: "Terminal",
                    level: 0,
                    frame: .init(x: 0, y: 0, width: 900, height: 700)
                ),
            ],
            onscreenWindowOrder: [9001],
            focusedWindowID: 9001,
            stableAppIDsByPID: [
                41: "com.apple.Terminal",
            ]
        ))

        let context = try backend.prepareFastFocusContext()

        XCTAssertEqual(context.environment, .validated)
        XCTAssertEqual(context.desktopSnapshot.windows.first?.pid, 41)
        XCTAssertEqual(context.desktopSnapshot.windows.first?.appID, "com.apple.Terminal")
        XCTAssertEqual(context.desktopSnapshot.windows.first?.title, "Terminal")
    }

    func testPrepareFastFocusContextDoesNotQueryMainConnectionOrWindowDescriptions() throws {
        let system = FastFocusRecordingSystem(
            requiredSymbols: PrivateSymbols.fastFocusRequiredSymbols,
            accessibilityTrusted: true,
            mainConnectionID: nil,
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
                101: [9001],
                102: [9002, 9003],
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
                    level: 0,
                    frame: .init(x: 10, y: 10, width: 900, height: 700)
                ),
                9003: .init(
                    id: 9003,
                    pid: 43,
                    title: "Notes",
                    level: 0,
                    frame: .init(x: 20, y: 20, width: 900, height: 700)
                ),
            ],
            onscreenWindowOrder: [9001],
            focusedWindowID: 9001,
            stableAppIDsByPID: [
                41: "com.apple.Terminal",
                42: "com.apple.Safari",
                43: "com.apple.Notes",
            ]
        )
        let backend = Backend(system: system)

        let context = try backend.prepareFastFocusContext()

        XCTAssertEqual(system.managedDisplaySpacesCallCount, 0)
        XCTAssertEqual(system.mainConnectionIDCallCount, 0)
        XCTAssertEqual(system.windowsForSpaceCalls, [])
        XCTAssertEqual(system.windowDescriptionRequests, [])
        XCTAssertEqual(system.focusedWindowDetailsCallCount, 1)
        XCTAssertEqual(context.desktopSnapshot.focusedWindowID, 1)
        XCTAssertEqual(context.desktopSnapshot.windows.first?.pid, 41)
    }

    func testPrepareFastFocusContextFallsBackWhenFocusedWindowDetailsProbeFails() throws {
        let system = FastFocusRecordingSystem(
            requiredSymbols: PrivateSymbols.fastFocusRequiredSymbols,
            accessibilityTrusted: true,
            mainConnectionID: nil,
            managedDisplaySpaces: [],
            windowsBySpaceID: [:],
            windowDescriptionsByID: [
                9001: .init(
                    id: 9001,
                    pid: 41,
                    title: "Terminal",
                    level: 0,
                    frame: .init(x: 0, y: 0, width: 900, height: 700)
                ),
            ],
            onscreenWindowOrder: [9001],
            focusedWindowID: 9001,
            stableAppIDsByPID: [
                41: "com.apple.Terminal",
            ],
            focusedWindowDetailsError: BackendError.missingTopology("AXUIElementCopyAttributeValue")
        )
        let backend = Backend(system: system)

        let context = try backend.prepareFastFocusContext()

        XCTAssertEqual(system.focusedWindowDetailsCallCount, 1)
        XCTAssertEqual(system.windowDescriptionRequests, [[9001]])
        XCTAssertEqual(context.desktopSnapshot.focusedWindowID, 1)
        XCTAssertEqual(context.desktopSnapshot.windows.first?.pid, 41)
        XCTAssertEqual(context.desktopSnapshot.windows.first?.appID, "com.apple.Terminal")
    }

    func testPrepareFastFocusContextFallsBackToTopologySnapshotWhenFocusedWindowProbesFail() throws {
        let system = FastFocusRecordingSystem(
            requiredSymbols: PrivateSymbols.fastFocusRequiredSymbols,
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
                    ],
                ],
            ],
            windowsBySpaceID: [
                101: [9001],
            ],
            windowDescriptionsByID: [
                9001: .init(
                    id: 9001,
                    pid: 41,
                    title: "Terminal",
                    level: 0,
                    frame: .init(x: 0, y: 0, width: 900, height: 700)
                ),
            ],
            onscreenWindowOrder: [9001],
            focusedWindowID: 9001,
            stableAppIDsByPID: [
                41: "com.apple.Terminal",
            ],
            focusedWindowDetailsError: BackendError.missingTopology("AXUIElementCopyAttributeValue"),
            focusedWindowIDError: BackendError.missingTopology("AXUIElementCopyAttributeValue")
        )
        let backend = Backend(system: system)

        let context = try backend.prepareFastFocusContext()

        XCTAssertEqual(system.focusedWindowDetailsCallCount, 1)
        XCTAssertEqual(system.mainConnectionIDCallCount, 0)
        XCTAssertEqual(system.managedDisplaySpacesCallCount, 1)
        XCTAssertEqual(system.windowsForSpaceCalls, [101])
        XCTAssertEqual(system.windowDescriptionRequests, [[9001]])
        XCTAssertNil(context.desktopSnapshot.focusedWindowID)
        XCTAssertEqual(context.desktopSnapshot.windows.first?.pid, 41)
        XCTAssertEqual(context.desktopSnapshot.windows.first?.appID, "com.apple.Terminal")
    }
}
