import Foundation
import MacosWindowManagerFFI
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
        XCTAssertTrue(exports.contains("@_cdecl(\"mwm_status_release\")"))
        XCTAssertTrue(exports.contains("@_cdecl(\"mwm_desktop_snapshot_release\")"))
    }

    func testTransportContractDocumentsLayoutAndOwnershipGuards() throws {
        let packageRoot = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
        let transportURL = packageRoot
            .appendingPathComponent("Sources")
            .appendingPathComponent("MacosWindowManagerFFI")
            .appendingPathComponent("Transport.swift")
        let transport = try String(contentsOf: transportURL, encoding: .utf8)

        XCTAssertTrue(
            transport.contains("verifyTransportAbiContract"),
            "Swift transport should assert the ABI layout it shares with Rust"
        )
        XCTAssertTrue(
            transport.contains("mwm_status_release")
                && transport.contains("mwm_desktop_snapshot_release"),
            "Swift transport should document how owned payload pointers are released"
        )
    }

    func testTransportAbiLayoutMatchesRustContract() {
        XCTAssertEqual(MemoryLayout<MwmStatus>.stride, 16)
        XCTAssertEqual(MemoryLayout<MwmStatus>.alignment, 8)
        XCTAssertEqual(MemoryLayout<MwmStatus>.offset(of: \.code), .some(0))
        XCTAssertEqual(MemoryLayout<MwmStatus>.offset(of: \.message_ptr), .some(8))

        XCTAssertEqual(MemoryLayout<MwmRectAbi>.stride, 16)
        XCTAssertEqual(MemoryLayout<MwmRectAbi>.alignment, 4)
        XCTAssertEqual(MemoryLayout<MwmRectAbi>.offset(of: \.x), .some(0))
        XCTAssertEqual(MemoryLayout<MwmRectAbi>.offset(of: \.y), .some(4))
        XCTAssertEqual(MemoryLayout<MwmRectAbi>.offset(of: \.width), .some(8))
        XCTAssertEqual(MemoryLayout<MwmRectAbi>.offset(of: \.height), .some(12))

        XCTAssertEqual(MemoryLayout<MwmSpaceAbi>.stride, 24)
        XCTAssertEqual(MemoryLayout<MwmSpaceAbi>.alignment, 8)
        XCTAssertEqual(MemoryLayout<MwmSpaceAbi>.offset(of: \.id), .some(0))
        XCTAssertEqual(MemoryLayout<MwmSpaceAbi>.offset(of: \.display_index), .some(8))
        XCTAssertEqual(MemoryLayout<MwmSpaceAbi>.offset(of: \.active), .some(16))
        XCTAssertEqual(MemoryLayout<MwmSpaceAbi>.offset(of: \.kind), .some(20))

        XCTAssertEqual(MemoryLayout<MwmWindowAbi>.stride, 80)
        XCTAssertEqual(MemoryLayout<MwmWindowAbi>.alignment, 8)
        XCTAssertEqual(MemoryLayout<MwmWindowAbi>.offset(of: \.id), .some(0))
        XCTAssertEqual(MemoryLayout<MwmWindowAbi>.offset(of: \.pid), .some(8))
        XCTAssertEqual(MemoryLayout<MwmWindowAbi>.offset(of: \.has_pid), .some(12))
        XCTAssertEqual(MemoryLayout<MwmWindowAbi>.offset(of: \.app_id_ptr), .some(16))
        XCTAssertEqual(MemoryLayout<MwmWindowAbi>.offset(of: \.title_ptr), .some(24))
        XCTAssertEqual(MemoryLayout<MwmWindowAbi>.offset(of: \.frame), .some(32))
        XCTAssertEqual(MemoryLayout<MwmWindowAbi>.offset(of: \.has_frame), .some(48))
        XCTAssertEqual(MemoryLayout<MwmWindowAbi>.offset(of: \.level), .some(52))
        XCTAssertEqual(MemoryLayout<MwmWindowAbi>.offset(of: \.space_id), .some(56))
        XCTAssertEqual(MemoryLayout<MwmWindowAbi>.offset(of: \.order_index), .some(64))
        XCTAssertEqual(MemoryLayout<MwmWindowAbi>.offset(of: \.has_order_index), .some(72))

        XCTAssertEqual(MemoryLayout<MwmDesktopSnapshotAbi>.stride, 40)
        XCTAssertEqual(MemoryLayout<MwmDesktopSnapshotAbi>.alignment, 8)
        XCTAssertEqual(MemoryLayout<MwmDesktopSnapshotAbi>.offset(of: \.spaces_ptr), .some(0))
        XCTAssertEqual(MemoryLayout<MwmDesktopSnapshotAbi>.offset(of: \.spaces_len), .some(8))
        XCTAssertEqual(MemoryLayout<MwmDesktopSnapshotAbi>.offset(of: \.windows_ptr), .some(16))
        XCTAssertEqual(MemoryLayout<MwmDesktopSnapshotAbi>.offset(of: \.windows_len), .some(24))
        XCTAssertEqual(MemoryLayout<MwmDesktopSnapshotAbi>.offset(of: \.focused_window_id), .some(32))
    }
}
