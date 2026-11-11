// His answers on disk: what he has marked and written outlives the window.
//
// «блядь сука а хули у меня вклинившаяся проверка с мобилы сбросила нахуй всё что я наотвечал». His answers lived
// in this process's memory until a check was ended. A window of a newer version took this one's place while a
// check he had half answered stood aside for the phone's, and they were gone. Now every answer and every letter
// of a comment is written down as it is given, and a window that comes up finds them: the same check, the same
// item, the comment as he left it.

import AppKit

/// What is written down: the check that is on the window, and his answers by check.
struct Answers: Codable {
    var active: String?
    var checks: [String: Controller.Kept]
}

extension Controller {
    /// What he has answered and not yet sent: the check on the window as it stands now, and those set aside.
    /// A check he has not touched has nothing to keep.
    private var unsent: Answers {
        var all = kept
        if let ticket = active { all[ticket.id] = Kept(marks: marks, index: index, shownAt: shownAt) }
        all = all.filter { entry in
            entry.value.marks.contains { $0.state != nil || !$0.note.isEmpty || $0.values != nil || !($0.shots ?? []).isEmpty }
        }
        return Answers(active: active.flatMap { all[$0.id] == nil ? nil : $0.id }, checks: all)
    }

    /// Writes his answers down when they are not what was last written; with none to keep, nothing is left.
    func saveAnswers() {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        let all = unsent
        guard let data = try? encoder.encode(all), data != answersSaid else { return }
        answersSaid = data
        if all.checks.isEmpty {
            try? FileManager.default.removeItem(at: answersFile)
        } else {
            try? FileManager.default.createDirectory(at: windowDirectory, withIntermediateDirectories: true)
            try? data.write(to: answersFile, options: .atomic)
        }
    }

    /// What the window before this one had of his answers. The checks that no longer wait drop out with the
    /// first look at the queue; the one he was at comes up again, where he left it.
    func recallAnswers() -> String? {
        guard let data = try? Data(contentsOf: answersFile), let all = try? JSONDecoder().decode(Answers.self, from: data) else { return nil }
        kept = all.checks
        return all.active
    }
}
