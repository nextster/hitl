# An iPhone build check

Use `hitl iphone --check` to put a build on a shared iPhone with a short checklist. See `hitl iphone --help` for build and checklist options. Name the exact behavior to try in each item.

The queue handles the build, waits its turn, and returns the person's marks when the check ends. A green check records approval for the behavior it covered.

Use `hitl queue` to see who is waiting. Use `hitl wait <id>` to retrieve a completed check when the answer has not arrived. Use `hitl drop` to take back your own check.
