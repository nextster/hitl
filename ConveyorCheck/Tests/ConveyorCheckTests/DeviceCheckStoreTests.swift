import Foundation
import Testing
@testable import ConveyorCheck

/// Defaults that live as long as one test and are written nowhere: a suite of
/// real ones leaves its file among the user's settings, and emptied it stays.
private final class DefaultsOfOneTest: UserDefaults, @unchecked Sendable {
    private var kept: [String: Any] = [:]

    override func object(forKey name: String) -> Any? { kept[name] }
    override func data(forKey name: String) -> Data? { kept[name] as? Data }
    override func set(_ value: Any?, forKey name: String) { kept[name] = value }
    override func removeObject(forKey name: String) { kept[name] = nil }
}

/// The hand check a host installs a build for: the checklist arrives with
/// one launch, stays for the launches by hand, and the marks reach the file
/// the host reads.
@MainActor @Suite final class DeviceCheckStoreTests {
    /// What the host passed for a three-item check; `DeviceCheckTests` holds
    /// the same value.
    private static let argument = "eyJpZCI6IjEwMDctMTQxNTAyLTctcjM4IiwidGFnIjoici8zOCIsInRpdGxlIjoi0JHQsNGA0Ysg0LLQvdC40LfRgzog0L_QtdGA0LXQvdC-0YEg0Lgg0LTQstC40LbQtdC90LjQtSIsIml0ZW1zIjpbeyJsYWJlbCI6IjEiLCJ0ZXh0Ijoi0KHQstCw0LnQv9C90Lgg0LzQtdC20LTRgyDRjdC60YDQsNC90LDQvNC4IOKGkiDRhNGD0YLQtdGAINC80LXQvdGP0LXRgtGB0Y8g0L_Qu9Cw0LLQvdC-In0seyJsYWJlbCI6IjRhIiwidGV4dCI6ItCX0LDQttC80Lgg0YfQuNC_IMKr0JTQvtC8wrsg4oaSINC-0L0g0LXQtNC10YIg0LfQsCDQv9Cw0LvRjNGG0LXQvCJ9LHsibGFiZWwiOiI1IiwidGV4dCI6ItCo0LXQstGA0L7QvdGLINGE0LXQudC00Y_RgtGB0Y8g0LIg0L3QsNGH0LDQu9C1In1dfQ"

    private let defaults: UserDefaults = DefaultsOfOneTest()
    private let directory = FileManager.default.temporaryDirectory.appendingPathComponent("device-check-store-\(UUID().uuidString)")

    /// Nothing of a test stays on the machine that ran it: each one left a
    /// folder and a file of settings behind, and hundreds of both had piled up.
    isolated deinit {
        try? FileManager.default.removeItem(at: directory)
    }

    /// One launch: by the host with the checklist, by the host without one,
    /// or by hand, which passes nothing.
    private func launch(_ arguments: [String] = [], installation: String = "install-1") -> DeviceCheckStore {
        DeviceCheckStore(defaults: defaults, launch: DeviceCheckLaunch(arguments: arguments),
                         installation: installation, directory: directory, now: { Date(timeIntervalSince1970: 1_791_380_000) })
    }

    private var hostLaunch: [String] { ["--ios-run-tag", "r/38 · nextster/dock", "--ios-run-checklist", Self.argument] }

    /// What the host put beside the report.
    private func writeHost(_ json: String) throws {
        let url = directory.appendingPathComponent(DeviceCheckHost.relativePath)
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try Data(json.utf8).write(to: url)
    }

    private func file() throws -> [String: Any] {
        let data = try Data(contentsOf: directory.appendingPathComponent(DeviceCheckReport.relativePath))
        return try #require(JSONSerialization.jsonObject(with: data) as? [String: Any])
    }

    @Test func theHostsLaunchBringsTheChecklistAndStartsTheReportFile() throws {
        let store = launch(hostLaunch)
        #expect(store.isActive)
        #expect(store.arrivedWithThisLaunch)
        #expect(store.check?.tag == "r/38")
        #expect(store.check?.items.map(\.label) == ["1", "4a", "5"])
        let file = try file()
        #expect(file["id"] as? String == "1007-141502-7-r38")
        #expect(file["finishedAt"] == nil)
    }

    @Test func marksSurviveReopeningTheAppByHand() throws {
        let store = launch(hostLaunch)
        store.mark("1", .ok)
        store.mark("4a", .problem)
        store.note("4a", "уползает вправо")
        store.note("5", "не смотрел: нет тем")

        let reopened = launch()
        #expect(reopened.isActive)
        #expect(!reopened.arrivedWithThisLaunch)
        #expect(reopened.report == store.report)
        #expect(reopened.report?.result(of: "4a")?.state == .problem)
        #expect(reopened.report?.result(of: "4a")?.note == "уползает вправо")
        let results = try #require(try file()["results"] as? [[String: Any]])
        #expect(results.map { $0["state"] as? String } == ["ok", "problem", nil])
        #expect(results.map { $0["note"] as? String } == [nil, "уползает вправо", "не смотрел: нет тем"])
    }

    @Test func theOneAtATimeViewComesBackToTheItemItWasLeftAt() {
        let store = launch(hostLaunch)
        #expect(store.currentLabel == "1")
        store.select("5")
        store.select("no such item")
        #expect(store.currentLabel == "5")
        #expect(launch().currentLabel == "5")
        #expect(launch(hostLaunch).currentLabel == "5")
    }

    @Test func aRepeatedLaunchOfTheSameCheckKeepsItsMarks() {
        launch(hostLaunch).mark("1", .ok)
        #expect(launch(hostLaunch).report?.result(of: "1")?.state == .ok)
    }

    @Test func doneEndsTheCheckInTheFileAndTakesTheChecklistAway() throws {
        let store = launch(hostLaunch)
        store.mark("1", .skipped)
        store.finish()
        #expect(!store.isActive)
        #expect(try file()["finishedAt"] as? String == "2026-10-07T13:33:20Z")
        store.mark("1", .problem)
        #expect(store.report?.result(of: "1")?.state == .skipped)
        #expect(!launch().isActive)
    }

    @Test func aLaunchByTheHostWithoutAChecklistDropsTheOldOne() {
        launch(hostLaunch).mark("1", .ok)
        #expect(!launch(["--ios-run-tag", "r/41 · nextster/other"]).isActive)
        #expect(!launch().isActive, "Dropped for good, not hidden for one launch")
    }

    @Test func anotherInstallationDoesNotInheritTheChecklist() {
        launch(hostLaunch).mark("1", .ok)
        #expect(!launch(installation: "install-2").isActive)
        #expect(!launch().isActive)
    }

    @Test func whatTheTesterDoesOnTheHostsScreenReachesTheChecklist() throws {
        let store = launch(hostLaunch)
        store.mark("1", .ok)
        try writeHost(#"{"id":"1007-141502-7-r38","results":[{"label":"4a","state":"problem","note":"с Мака","at":1791380100}]}"#)
        store.readHost()
        #expect(store.report?.result(of: "4a") == .init(label: "4a", state: .problem, note: "с Мака", at: 1_791_380_100))
        let results = try #require(try file()["results"] as? [[String: Any]])
        #expect(results.map { $0["state"] as? String } == ["ok", "problem", nil])
        // The item the tester is at goes to the host with the marks.
        store.select("5")
        #expect(try file()["current"] as? String == "5")
        #expect(launch().report?.result(of: "4a")?.note == "с Мака")
    }

    @Test func theChecklistGoesToTheItemTheTesterCameToAtTheHost() throws {
        let store = launch(hostLaunch)
        #expect(store.currentLabel == "1")
        try writeHost(#"{"id":"1007-141502-7-r38","current":"5","currentAt":1791380100}"#)
        store.readHost()
        #expect(store.currentLabel == "5")
        #expect(launch().currentLabel == "5")
        // A move made here after that is the later one, whatever the two clocks say: the host's file, read
        // again, does not take it back, and the report says where the host is to go next.
        store.select("4a")
        #expect(try file()["current"] as? String == "4a")
        #expect(try file()["currentAt"] as? Double ?? 0 > 1_791_380_100)
        store.readHost()
        #expect(store.currentLabel == "4a")
    }

    @Test func aCheckEndedAtTheHostEndsHereToo() throws {
        let store = launch(hostLaunch)
        var told = 0
        store.finishedByHost = { told += 1 }
        try writeHost(#"{"id":"another-check","finished":true}"#)
        store.readHost()
        #expect(store.isActive)

        try writeHost(#"{"id":"1007-141502-7-r38","results":[{"label":"1","state":"ok","at":1791380100}],"finished":true}"#)
        store.readHost()
        #expect(!store.isActive)
        #expect(told == 1)
        #expect(store.report?.result(of: "1")?.state == .ok)
        #expect(try file()["finishedAt"] != nil)
        store.readHost()
        #expect(told == 1)
    }

    @Test func theReportSaysWhatThePanelOffersToDoForAnItem() throws {
        let store = launch(hostLaunch)
        #expect(try file()["offers"] == nil)
        store.offer([.init(label: "1", actions: [.init(id: ">setup", title: "Подготовить"), .init(id: "dock-fold", title: "dock-fold")]),
                     .init(label: "4a", actions: [])])
        // An item with nothing to offer is left out; the offers stay for a launch by hand.
        let offers = try #require(try file()["offers"] as? [[String: Any]])
        #expect(offers.map { $0["label"] as? String } == ["1"])
        let actions = try #require(offers.first?["actions"] as? [[String: String]])
        #expect(actions == [["id": ">setup", "title": "Подготовить"], ["id": "dock-fold", "title": "dock-fold"]])
        #expect(launch().report?.offers?.first?.actions.map(\.id) == [">setup", "dock-fold"])
    }

    @Test func theReportSaysWhatKindEachButtonIsAndHowItStands() throws {
        // «Просто дай сущность тумблера в возможные варианты для агентов. И подумай какие ещё нужны сущности».
        let store = launch(hostLaunch)
        store.offer([.init(label: "1", actions: [
            .init(id: "open-main", title: "/screen/main", kind: .link),
            .init(id: "dock-fold", title: "Дока раскрыта", kind: .toggle, symbol: "dock.rectangle", on: true),
            .init(id: "range-month", title: "Лента", kind: .choice,
                  options: [.init(id: "range-week", title: "Неделя"), .init(id: "range-month", title: "Месяц")], current: "range-week"),
            .init(id: "sign-out", title: "Выйти из аккаунта", kind: .action, symbol: "rectangle.portrait.and.arrow.right"),
            .init(id: "wait", title: "wait"),
        ])])
        let actions = try #require((try file()["offers"] as? [[String: Any]])?.first?["actions"] as? [[String: Any]])
        #expect(actions.map { $0["kind"] as? String } == ["link", "toggle", "choice", "action", nil])
        #expect(actions[1]["on"] as? Bool == true)
        #expect(actions[1]["symbol"] as? String == "dock.rectangle")
        #expect(actions[2]["current"] as? String == "range-week")
        #expect((actions[2]["options"] as? [[String: String]])?.map { $0["id"] } == ["range-week", "range-month"])
        // A button of plain words says nothing more than it did: an older host reads the same file.
        #expect(Set(actions[4].keys) == ["id", "title"])
    }

    @Test func whatStoodBeforeTheCheckIsKeptOnceAndOutlivesALaunchByHand() {
        // «Изменения например темы не должны сохраняться они эфемерные».
        let store = launch(hostLaunch)
        store.remember("dark", "off")
        // The check's own doing is not what stood before it.
        store.remember("dark", "on")
        store.remember("range-day", "range-week")
        #expect(store.remembered == ["dark": "off", "range-day": "range-week"])
        #expect(launch().remembered == ["dark": "off", "range-day": "range-week"])
        // The host launches the same check again: still the same check, and what stood before it.
        #expect(launch(hostLaunch).remembered == ["dark": "off", "range-day": "range-week"])
        // Another install is another copy of the app: nothing of the old check is its to put back.
        #expect(launch(installation: "install-2").remembered.isEmpty)
    }

    @Test func aCheckEndedAtTheHostPutsThingsBackBeforeTheReportSaysItIsOver() async throws {
        let store = launch(hostLaunch)
        store.remember("dark", "off")
        var order: [String] = []
        store.beforeFinish = {
            // The host may replace the app once the report says the check is over: not yet.
            order.append(store.report?.isFinished == true ? "back after the end" : "back")
            try? await Task.sleep(for: .milliseconds(5))
        }
        store.finishedByHost = { order.append("told") }
        try writeHost(#"{"id":"1007-141502-7-r38","finished":true}"#)
        store.readHost()
        // The file says so at every look until the check is over: things are put back once.
        store.readHost()
        #expect(store.report?.isFinished == false)
        await settle { store.report?.isFinished == true }
        store.readHost()
        await settle { false }
        #expect(order == ["back", "told"])
        #expect(store.report?.isFinished == true)
    }

    @Test func aButtonPressedAtTheHostRunsHereOnceAndTheReportSaysHowItWent() async throws {
        let store = launch(hostLaunch)
        var ran: [String] = []
        store.runForHost = { label, action in
            ran.append("\(label) \(action)")
            return action == "fail" ? "не вышло" : nil
        }
        try writeHost(#"{"id":"1007-141502-7-r38","run":{"label":"4a","action":">setup","at":1791380100}}"#)
        store.readHost()
        // The file says the same for as long as nothing else is pressed: one press is run once.
        store.readHost()
        await settle { store.report?.ran != nil }
        #expect(ran == ["4a >setup"])
        #expect(store.report?.ran == .init(at: 1_791_380_100, failure: nil))
        store.readHost()
        #expect(ran == ["4a >setup"])

        try writeHost(#"{"id":"1007-141502-7-r38","run":{"label":"5","action":"fail","at":1791380101.5}}"#)
        store.readHost()
        await settle { store.report?.ran?.at == 1_791_380_101.5 }
        #expect(ran == ["4a >setup", "5 fail"])
        let said = try #require(try file()["ran"] as? [String: Any])
        #expect(said["at"] as? Double == 1_791_380_101.5)
        #expect(said["failure"] as? String == "не вышло")
        // Reopened by hand, the app does not run again what it has run.
        let reopened = launch()
        reopened.runForHost = { label, action in
            ran.append("again \(label) \(action)")
            return nil
        }
        reopened.readHost()
        await settle { false }
        #expect(ran == ["4a >setup", "5 fail"])
    }

    @Test func aPressMadeBeforeThisLaunchIsNotRunByIt() async throws {
        // The host's file outlives a reinstall of the app: what was pressed for the copy before is not for this one.
        try writeHost(#"{"id":"1007-141502-7-r38","run":{"label":"1","action":"sign-out","at":1791380100}}"#)
        let store = launch(hostLaunch)
        var ran: [String] = []
        store.runForHost = { _, action in
            ran.append(action)
            return nil
        }
        store.listen(every: 1000)
        store.readHost()
        await settle { false }
        #expect(ran.isEmpty)
        try writeHost(#"{"id":"1007-141502-7-r38","run":{"label":"1","action":"open-main","at":1791380200}}"#)
        store.readHost()
        await settle { !ran.isEmpty }
        #expect(ran == ["open-main"])
    }

    /// Lets the main actor's tasks run until `done`, for a moment at most.
    private func settle(_ done: () -> Bool) async {
        for _ in 0..<50 where !done() { try? await Task.sleep(for: .milliseconds(2)) }
    }

    @Test func aLaunchWithoutAnyCheckShowsNothingAndWritesNothing() {
        #expect(!launch().isActive)
        #expect(!FileManager.default.fileExists(atPath: directory.appendingPathComponent(DeviceCheckReport.relativePath).path))
    }
}
