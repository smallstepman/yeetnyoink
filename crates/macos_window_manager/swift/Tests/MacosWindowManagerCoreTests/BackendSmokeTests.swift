import MacosWindowManagerCore
import XCTest

final class BackendSmokeTests: XCTestCase {
    func testPackageSmoke() {
        XCTAssertEqual(Backend.smokeTest(), 0)
    }
}
