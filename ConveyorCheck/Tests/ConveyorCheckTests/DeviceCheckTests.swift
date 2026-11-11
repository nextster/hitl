import Foundation
import Testing
@testable import ConveyorCheck

/// The hand check a host puts on a shared device. The host is not this
/// package, so its two formats are pinned here by value: the argument it sends
/// and the report it reads back.
@Suite struct DeviceCheckTests {
    /// What the host passed for a three-item check, verbatim.
    private static let argument = "eyJpZCI6IjEwMDctMTQxNTAyLTctcjM4IiwidGFnIjoici8zOCIsInRpdGxlIjoi0JHQsNGA0Ysg0LLQvdC40LfRgzog0L_QtdGA0LXQvdC-0YEg0Lgg0LTQstC40LbQtdC90LjQtSIsIml0ZW1zIjpbeyJsYWJlbCI6IjEiLCJ0ZXh0Ijoi0KHQstCw0LnQv9C90Lgg0LzQtdC20LTRgyDRjdC60YDQsNC90LDQvNC4IOKGkiDRhNGD0YLQtdGAINC80LXQvdGP0LXRgtGB0Y8g0L_Qu9Cw0LLQvdC-In0seyJsYWJlbCI6IjRhIiwidGV4dCI6ItCX0LDQttC80Lgg0YfQuNC_IMKr0JTQvtC8wrsg4oaSINC-0L0g0LXQtNC10YIg0LfQsCDQv9Cw0LvRjNGG0LXQvCJ9LHsibGFiZWwiOiI1IiwidGV4dCI6ItCo0LXQstGA0L7QvdGLINGE0LXQudC00Y_RgtGB0Y8g0LIg0L3QsNGH0LDQu9C1In1dfQ"

    private static let check = DeviceCheck(id: "1007-141502-7-r38", tag: "r/38", title: "Бары внизу: перенос и движение", items: [
        .init(label: "1", text: "Свайпни между экранами → футер меняется плавно"),
        .init(label: "4a", text: "Зажми чип «Дом» → он едет за пальцем"),
        .init(label: "5", text: "Шевроны фейдятся в начале"),
    ])

    private static let start = Date(timeIntervalSince1970: 1_791_380_000)

    @Test func theHostsArgumentDecodesWithItsNumbersAndText() {
        #expect(DeviceCheck(launchArgument: Self.argument) == Self.check)
    }

    /// An item may carry what the app can do for it: actions that prepare the
    /// screen, actions offered as buttons, a deep link. The host leaves out
    /// what an item does not have.
    @Test func itemsCarryTheActionsAndTheLinkTheHostSends() {
        let argument = "eyJpZCI6IjEwMDctMTgwMDAwLTktcjM4IiwidGFnIjoici8zOCIsInRpdGxlIjoi0J3QsNGB0YLRgNC-0LnQutC4IiwiaXRlbXMiOlt7ImxhYmVsIjoiMSIsInRleHQiOiLQntGC0LrRgNC-0Lkg0L3QsNGB0YLRgNC-0LnQutC4IOKGkiDQstC40LTQvdCwINGB0YLRgNC-0LrQsCDRj9C30YvQutCwIiwic2V0dXAiOlsib3Blbi1zZXR0aW5ncyJdLCJidXR0b25zIjpbImxhbmd1YWdlLWVuZ2xpc2giLCJsYW5ndWFnZS1ydXNzaWFuIl19LHsibGFiZWwiOiIyIiwidGV4dCI6ItCe0YLQutGA0L7QuSDRjdC60YDQsNC9INGB0L7Qt9C00LDQvdC40Y8iLCJsaW5rIjoicmVhbGl6ZTovL3NjcmVlbi90YXNrLWNyZWF0aW9uIn0seyJsYWJlbCI6IjMiLCJ0ZXh0Ijoi0JHQtdC3INC00LXQudGB0YLQstC40LkifV19"
        #expect(DeviceCheck(launchArgument: argument) == DeviceCheck(id: "1007-180000-9-r38", tag: "r/38", title: "Настройки", items: [
            .init(label: "1", text: "Открой настройки → видна строка языка", setup: ["open-settings"],
                  buttons: ["language-english", "language-russian"]),
            .init(label: "2", text: "Открой экран создания", link: "realize://screen/task-creation"),
            .init(label: "3", text: "Без действий"),
        ]))
    }

    /// A host may say which project the check is about; one that does not is read as before.
    @Test func theProjectIsReadWhenTheHostSaysIt() throws {
        let said = #"{"id":"1011-153000-1-r38","tag":"r/38","project":"realize","title":"Бары","items":[{"label":"1","text":"Свайпни"}]}"#
        let argument = Data(said.utf8).base64EncodedString().replacingOccurrences(of: "+", with: "-")
            .replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "=", with: "")
        #expect(DeviceCheck(launchArgument: argument) == DeviceCheck(id: "1011-153000-1-r38", tag: "r/38", project: "realize", title: "Бары",
                                                                     items: [.init(label: "1", text: "Свайпни")]))
        #expect(DeviceCheck(launchArgument: Self.argument)?.project == nil)
    }

    @Test func anArgumentThatIsNotACheckIsIgnored() {
        #expect(DeviceCheck(launchArgument: "") == nil)
        #expect(DeviceCheck(launchArgument: "not base64 at all") == nil)
        // Valid JSON without items is not something to check.
        #expect(DeviceCheck(launchArgument: Data(#"{"id":"x","tag":"r/1","title":"","items":[]}"#.utf8).base64EncodedString()) == nil)
        #expect(DeviceCheck(launchArgument: Data(#"{"id":"","tag":"r/1","title":"","items":[{"label":"1","text":"a"}]}"#.utf8).base64EncodedString()) == nil)
    }

    @Test func marksAndNotesAreKeptByTheItemsNumber() {
        var report = DeviceCheckReport(check: Self.check, now: Self.start)
        #expect(report.results.map(\.label) == ["1", "4a", "5"])
        #expect(report.marked == 0)

        let later = Self.start.addingTimeInterval(30)
        report.mark("4a", .problem, now: later)
        report.note("4a", "уползает вправо", now: later)
        report.mark("1", .ok, now: later)
        report.mark("no such item", .ok, now: later)
        #expect(report.result(of: "4a")?.state == .problem)
        #expect(report.result(of: "4a")?.note == "уползает вправо")
        // Each change is later than the one before it, though the clock of this test stands still.
        #expect(report.result(of: "4a")?.at ?? 0 > later.timeIntervalSince1970)
        #expect(report.result(of: "1")?.at ?? 0 > report.result(of: "4a")?.at ?? 0)
        #expect(report.marked == 2)
        #expect(!report.isComplete)
        #expect(report.updatedAt == later)
        // A comment needs no mark: any item can carry one.
        report.note("5", "работает, но медленно", now: later)
        #expect(report.result(of: "5")?.state == nil)
        #expect(report.result(of: "5")?.note == "работает, но медленно")
        report.note("5", "", now: later)

        // The same mark again is not a change; clearing one is.
        report.mark("1", .ok, now: later.addingTimeInterval(5))
        #expect(report.updatedAt == later)
        report.mark("1", nil, now: later.addingTimeInterval(5))
        report.note("4a", "", now: later.addingTimeInterval(5))
        #expect(report.marked == 1)
        #expect(report.result(of: "4a")?.note == nil)
    }

    @Test func aFinishedCheckKeepsTheMarksItWasFinishedWith() {
        var report = DeviceCheckReport(check: Self.check, now: Self.start)
        report.mark("1", .ok, now: Self.start)
        let done = Self.start.addingTimeInterval(60)
        report.finish(now: done)
        #expect(report.isFinished)

        report.mark("1", .problem, now: done.addingTimeInterval(10))
        report.note("1", "поздно", now: done.addingTimeInterval(10))
        report.finish(now: done.addingTimeInterval(10))
        #expect(report.result(of: "1") == .init(label: "1", state: .ok, at: Self.start.timeIntervalSince1970))
        #expect(report.finishedAt == done)
    }

    /// The file the host copies off the device, byte for byte.
    @Test func theReportFileIsWhatTheHostReads() throws {
        var report = DeviceCheckReport(check: Self.check, now: Self.start)
        report.mark("1", .ok, now: Self.start)
        report.mark("4a", .problem, now: Self.start.addingTimeInterval(1))
        report.note("4a", "уползает вправо", now: Self.start.addingTimeInterval(2))
        report.mark("5", .skipped, now: Self.start.addingTimeInterval(3))
        #expect(report.isComplete)
        #expect(String(decoding: try report.fileData(), as: UTF8.self) == """
        {"current":"1","id":"1007-141502-7-r38","results":[{"at":1791380000,"label":"1","state":"ok"},\
        {"at":1791380002,"label":"4a","note":"уползает вправо","state":"problem"},{"at":1791380003,"label":"5","state":"skipped"}],\
        "updatedAt":"2026-10-07T13:33:23Z"}
        """)

        report.finish(now: Self.start.addingTimeInterval(100))
        let file = try #require(JSONSerialization.jsonObject(with: try report.fileData()) as? [String: Any])
        #expect(file["finishedAt"] as? String == "2026-10-07T13:35:00Z")
        #expect(file["id"] as? String == Self.check.id)
    }

    /// The same checklist may be open on the host's screen: of two marks of
    /// an item the later one stays, wherever it was made.
    @Test func marksMadeAtTheHostComeInAndTheLaterOneStays() {
        var report = DeviceCheckReport(check: Self.check, now: Self.start)
        let start = Self.start.timeIntervalSince1970
        report.mark("1", .ok, now: Self.start.addingTimeInterval(10))
        report.take(DeviceCheckHost(id: Self.check.id, results: [
            // Earlier than the device's own mark: dropped.
            .init(label: "1", state: .problem, note: "с Мака", at: start + 5),
            .init(label: "4a", state: .skipped, note: nil, at: start + 20),
            .init(label: "no such item", state: .ok, note: nil, at: start + 20),
        ]), now: Self.start.addingTimeInterval(21))
        #expect(report.result(of: "1")?.state == .ok)
        #expect(report.result(of: "4a") == .init(label: "4a", state: .skipped, at: start + 20))
        #expect(report.updatedAt == Self.start.addingTimeInterval(21))

        // A later word from the host replaces the device's, comment and all.
        report.take(DeviceCheckHost(id: Self.check.id, results: [.init(label: "1", state: .problem, note: "мигает", at: start + 30)]),
                    now: Self.start.addingTimeInterval(31))
        #expect(report.result(of: "1") == .init(label: "1", state: .problem, note: "мигает", at: start + 30))
        // A file left by another check is not this one's, and a finished check takes nothing.
        report.take(DeviceCheckHost(id: "another", results: [.init(label: "1", state: .ok, note: nil, at: start + 99)]),
                    now: Self.start.addingTimeInterval(100))
        #expect(report.result(of: "1")?.state == .problem)
        report.finish(now: Self.start.addingTimeInterval(110))
        report.take(DeviceCheckHost(id: Self.check.id, results: [.init(label: "1", state: .ok, note: nil, at: start + 120)]),
                    now: Self.start.addingTimeInterval(121))
        #expect(report.result(of: "1")?.state == .problem)
    }

    @Test func theItemTheTesterIsAtGoesToTheHost() throws {
        var report = DeviceCheckReport(check: Self.check, now: Self.start)
        #expect(report.current == "1")
        #expect(report.currentAt == nil)
        report.select("5", now: Self.start.addingTimeInterval(10))
        report.select("no such item", now: Self.start.addingTimeInterval(11))
        #expect(report.current == "5")
        let file = try #require(JSONSerialization.jsonObject(with: try report.fileData()) as? [String: Any])
        #expect(file["current"] as? String == "5")
        #expect(file["currentAt"] as? Double == Self.start.timeIntervalSince1970 + 10)
    }

    /// The tester moves through the checklist on either screen: the later move is where both are.
    @Test func theItemTheTesterCameToAtTheHostIsTheItemHere() {
        var report = DeviceCheckReport(check: Self.check, now: Self.start)
        let start = Self.start.timeIntervalSince1970
        report.take(DeviceCheckHost(id: Self.check.id, results: [], current: "4a", currentAt: start + 5), now: Self.start)
        #expect(report.current == "4a")
        report.select("5", now: Self.start.addingTimeInterval(20))
        // An older move of the host's, and one to an item that is not there, change nothing.
        report.take(DeviceCheckHost(id: Self.check.id, results: [], current: "1", currentAt: start + 10), now: Self.start)
        report.take(DeviceCheckHost(id: Self.check.id, results: [], current: "no such item", currentAt: start + 99), now: Self.start)
        #expect(report.current == "5")
        report.take(DeviceCheckHost(id: Self.check.id, results: [], current: "1", currentAt: start + 30), now: Self.start)
        #expect(report.current == "1")
        #expect(report.currentAt == start + 30)
    }

    /// The device's clock and the host's are two clocks: what is done after another thing was seen is later
    /// than it, whatever they say.
    @Test func aMoveMadeAfterTheHostsIsLaterThanItWhateverTheClocksSay() {
        var report = DeviceCheckReport(check: Self.check, now: Self.start)
        let start = Self.start.timeIntervalSince1970
        // The host's clock is a minute ahead of this device's.
        report.take(DeviceCheckHost(id: Self.check.id, results: [.init(label: "1", state: .problem, note: nil, at: start + 60)],
                                    current: "4a", currentAt: start + 61), now: Self.start.addingTimeInterval(1))
        #expect(report.current == "4a")
        report.mark("1", .ok, now: Self.start.addingTimeInterval(2))
        report.select("5", now: Self.start.addingTimeInterval(3))
        #expect(report.result(of: "1")?.at ?? 0 > start + 61)
        #expect(report.currentAt ?? 0 > report.result(of: "1")?.at ?? 0)
        // So the host's older word does not take either back.
        report.take(DeviceCheckHost(id: Self.check.id, results: [.init(label: "1", state: .problem, note: nil, at: start + 60)],
                                    current: "4a", currentAt: start + 61), now: Self.start.addingTimeInterval(4))
        #expect(report.result(of: "1")?.state == .ok)
        #expect(report.current == "5")
    }

    @Test func theHostsFileIsReadWithWhatItLeavesOut() throws {
        let json = #"{"id":"1007-141502-7-r38","results":[{"label":"1","state":"ok","at":1791380040.5},{"label":"5","note":"с Мака","at":1791380041}]}"#
        let host = try #require(DeviceCheckHost(fileData: Data(json.utf8)))
        #expect(host.results == [.init(label: "1", state: .ok, note: nil, at: 1_791_380_040.5),
                                 .init(label: "5", state: nil, note: "с Мака", at: 1_791_380_041)])
        #expect(!host.finished)
        #expect(host.current == nil)
        let moved = try #require(DeviceCheckHost(fileData: Data(#"{"id":"x","current":"4a","currentAt":1791380050.25}"#.utf8)))
        #expect(moved.current == "4a")
        #expect(moved.currentAt == 1_791_380_050.25)
        #expect(DeviceCheckHost(fileData: Data(#"{"id":"x","finished":true}"#.utf8))?.finished == true)
        #expect(DeviceCheckHost(fileData: Data("not json".utf8)) == nil)
    }
}
