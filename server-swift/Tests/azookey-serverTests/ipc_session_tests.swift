import Foundation
import XCTest
@testable import azookey_server

final class IpcSessionTests: XCTestCase {
    func testInputCursorContextAndNotationAreConnectionLocal() async {
        await MainActor.run {
            var cursor: Int32 = 0
            select_ipc_session(101)
            clear_text()
            let first = "privateA".withCString { append_text_direct(input: $0, cursorPtr: &cursor) }
            free_text(first)
            "contextA".withCString { set_context(context: $0) }
            plainNotation = true
            let moved = move_cursor(offset: -2, cursorPtr: &cursor)
            free_text(moved)
            let position = cursor

            select_ipc_session(102)
            XCTAssertTrue(composingText.convertTarget.isEmpty)
            XCTAssertEqual(config["context"] as? String, "")
            XCTAssertFalse(plainNotation)
            let second = "privateB".withCString { append_text_direct(input: $0, cursorPtr: &cursor) }
            free_text(second)
            "contextB".withCString { set_context(context: $0) }

            select_ipc_session(101)
            XCTAssertEqual(composingText.convertTarget, "privateA")
            XCTAssertEqual(composingText.convertTargetCursorPosition, Int(position))
            XCTAssertEqual(config["context"] as? String, "contextA")
            XCTAssertTrue(plainNotation)
            clear_text()
            select_ipc_session(102)
            XCTAssertEqual(composingText.convertTarget, "privateB")
            XCTAssertEqual(config["context"] as? String, "contextB")
            release_ipc_session(101)
            release_ipc_session(102)
        }
    }

    func testReleasedConnectionCannotRecoverPreviousInput() async {
        await MainActor.run {
            var cursor: Int32 = 0
            select_ipc_session(103)
            let text = "discarded".withCString { append_text_direct(input: $0, cursorPtr: &cursor) }
            free_text(text)
            release_ipc_session(103)
            select_ipc_session(103)
            XCTAssertTrue(composingText.convertTarget.isEmpty)
            XCTAssertEqual(config["context"] as? String, "")
            release_ipc_session(103)
        }
    }
}
