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
}
