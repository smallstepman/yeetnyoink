import Foundation
import XCTest

final class FFIContractTests: XCTestCase {
    func testRequiredExportsExist() throws {
        let packageRoot = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
        let exportsURL = packageRoot
            .appendingPathComponent("Sources")
            .appendingPathComponent("MacosWindowManagerFFI")
            .appendingPathComponent("Exports.swift")
        let exports = try String(contentsOf: exportsURL, encoding: .utf8)

        XCTAssertTrue(exports.contains("@_cdecl(\"mwm_backend_new\")"))
        XCTAssertTrue(exports.contains("@_cdecl(\"mwm_backend_free\")"))
        XCTAssertTrue(exports.contains("@_cdecl(\"mwm_backend_desktop_snapshot\")"))
    }
}
