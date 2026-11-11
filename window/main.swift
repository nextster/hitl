// Where the window starts: the lock that keeps it one, the file that says it is up, the app itself.
//
// CheckPanel: one floating window on the Mac for hand checks from every agent
// chat. `hitl check` compiles and starts it when the first check is queued;
// it shows the oldest ticket of ~/.conveyor/queue one item at a time, writes
// the answers to ~/.conveyor/done/<id>.json and exits when nothing waits.
// Nothing is installed in the system and nothing stays running.
//
// The window never takes the keyboard by itself: it appears over whatever he is
// typing in. A click on it gives it the keyboard, and from then on every answer
// has a key, written at the right of its row.
//
// The look is "graphite", chosen by the reviewer from two on 2026-10-08: a dense dark
// panel, the question large, the answers as rows, the actions in a bar below.
//
// The window has two shapes. Small, it is that panel. On the whole screen it is
// a page over everything, the Dock and the menu bar too: the check's items and
// the waiting checks listed at the left, the item in the middle, its pictures
// large at the right. A first whole screen was the same panel, a little larger,
// in the middle of a dark sheet: «это безобразие а не фуллскрин».
//
// A picture is looked at closer on a card in the middle of the screen, without
// leaving the window for another app: by a click on it or by its key, ⌘1 and on.

import AppKit
atexit {
    ActionJournal.shared.record("hitl.panel.exit", request: "", outcome: "exiting", reason: "atexit")
    ActionJournal.shared.close()
}

// One window at a time: the queue is one, and a second window would show the
// same check twice. The lock goes with the process.
try? FileManager.default.createDirectory(at: windowDirectory, withIntermediateDirectories: true)
let lock = open(windowDirectory.appendingPathComponent("panel.lock").path, O_CREAT | O_RDWR, 0o644)
if lock < 0 || flock(lock, LOCK_EX | LOCK_NB) != 0 {
    ActionJournal.shared.record("hitl.panel.start_refused", request: "", outcome: "refused", reason: "panel_lock")
    exit(0)
}
ActionJournal.shared.record("hitl.panel.start", request: "", outcome: "started", reason: "lock_acquired")
let running = Running(pid: getpid(), binary: Bundle.main.executablePath ?? CommandLine.arguments[0])
try? JSONEncoder().encode(running).write(to: windowDirectory.appendingPathComponent("panel.pid"), options: .atomic)

let application = NSApplication.shared
application.setActivationPolicy(.accessory)
let controller = Controller()
application.run()
ActionJournal.shared.record("hitl.panel.exit", request: "", outcome: "exiting", reason: "run_returned")
