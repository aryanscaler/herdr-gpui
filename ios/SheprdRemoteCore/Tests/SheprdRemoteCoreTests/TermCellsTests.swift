import Foundation
import Testing
@testable import SheprdRemoteCore

@Suite struct TermCellsTests {
    @Test func latinTakesACellEach() {
        #expect(TermScreen.cells("❯ hello") == 7)
        #expect(TermScreen.cells("") == 0)
    }

    @Test func chineseJapaneseAndKoreanTakeTwo() {
        // A program's cursor after them is drawn where the terminal put it.
        #expect(TermScreen.cells("好的回电话") == 10)
        #expect(TermScreen.cells("かなカナ한글") == 12)
        #expect(TermScreen.cells("，。\u{3000}") == 6)
        #expect(TermScreen.cells("ＡＢ") == 4)
    }

    @Test func emojiTakeTwo() {
        #expect(TermScreen.cells("👍") == 2)
        #expect(TermScreen.cells("❤️") == 2)
        #expect(TermScreen.cells("👨‍👩‍👧") == 2)
    }

    @Test func boxDrawingAndAccentsTakeOne() {
        #expect(TermScreen.cells("─│█") == 3)
        #expect(TermScreen.cells("é") == 1)
        #expect(TermScreen.cells("❯") == 1)
    }

    func screen(_ rows: [String]) throws -> TermScreen {
        let json = rows.map { #"[{"text":"\#($0)"}]"# }.joined(separator: ",")
        return try JSONDecoder().decode(TermScreen.self, from: Data(#"{"pane":"p","columns":20,"rows":[\#(json)]}"#.utf8))
    }

    @Test func aScreenHeldAtThePhonesSizeFitsItsColumnsWhateverItsRowsSpan() throws {
        // Scrolled back past an inline program's full-width rules, no row
        // on screen spans the terminal: its text keeps the size the rows
        // were counted at, so they all still fit the screen's height.
        let history = try screen(["正在将包含 c229bd7d52", "● Ran (ssh macmini-hq)"])
        #expect(history.fitColumns(held: TermSize(cols: 57, rows: 46)) == 57)
        // Not held, it fits its widest row.
        #expect(history.fitColumns(held: nil) == 22)
        // Still wider than the phone's size before the resize lands.
        let layout = try screen([String(repeating: "─", count: 60)])
        #expect(layout.fitColumns(held: TermSize(cols: 57, rows: 46)) == 60)
    }
}
