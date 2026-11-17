# An iPhone build check

Use `hitl iphone --check` to put a build on a shared iPhone with a short checklist. See `hitl iphone --help` for build and checklist options. Name the exact behavior to try in each item.

Use `--check-setup`, `--check-button` and `--check-link` only for actions the app on the iPhone can run. Clicking a phone action in the Mac mirror still sends it to the iPhone. For a PR, docs, or another web reference to open on the Mac, add `--check-mac-link URL` after the corresponding `--check` (or `mac-link: URL` in a checklist file). It is a clickable link in the Mac mirror only. Mac checks use the usual `--link`.

The queue handles the build, waits its turn, and returns the person's marks when the check ends. A green check records approval for the behavior it covered.

Use `hitl queue` to see who is waiting. Use `hitl wait <id>` to retrieve a completed check when the answer has not arrived. Use `hitl drop` to take back your own check.
