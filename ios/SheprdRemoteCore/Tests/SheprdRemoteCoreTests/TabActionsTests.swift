import Foundation
@testable import SheprdRemoteCore
import Testing

/// What a terminal tab's menu may do: a computer refuses to split, zoom,
/// or rename a Herdr desktop space's tab, and says so up front.
struct TabActionsTests {
    private func row(_ json: String) throws -> Home.TerminalRow {
        try JSONDecoder().decode(Home.TerminalRow.self, from: Data(json.utf8))
    }

    @Test func readsWhatTheComputerOffers() throws {
        let message = try JSONSerialization.jsonObject(with: WireTests.fixture("home")) as? [String: Any]
        let data = try JSONSerialization.data(withJSONObject: message?["home"] ?? [:])
        let home = try JSONDecoder().decode(Home.self, from: data)
        let terminal = home.workspaces[0].terminals[0]
        #expect(terminal.offers(.split) && terminal.offers(.zoom) && terminal.offers(.rename))
    }

    @Test func aDesktopSpaceTabOffersNone() throws {
        let terminal = try row(#"{"tab":"core:app-tab:61","label":"zsh","zoomed":false,"actions":[],"panes":[]}"#)
        #expect(!terminal.offers(.split))
        #expect(!terminal.offers(.zoom))
        #expect(!terminal.offers(.rename))
    }

    @Test func anOlderComputerAllowsThemAllAndANewActionIsRead() throws {
        let older = try row(#"{"tab":"t1","label":"zsh","zoomed":false,"panes":[]}"#)
        #expect(older.offers(.split))
        let newer = try row(#"{"tab":"t1","label":"zsh","zoomed":false,"actions":["zoom","detach"],"panes":[]}"#)
        #expect(newer.offers(.zoom))
        #expect(!newer.offers(.split))
    }
}
