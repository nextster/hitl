# While a check waits: how the answer reaches you

A part of the `hitl` skill: read it once you have queued your first check. `SKILL.md` beside it says what to do while a check waits; here is how the verdict comes to you in each app, and what the command may ask of you meanwhile. Its messages begin with its own name and a colon: «hitl:», or the name `local.md` gives it.

In Claude Code the command waits in the background and the app wakes you when it exits; the verdict is what it printed.

**The wake can be lost, so take your answers when you come back to work.** The app stops the command that waits with the chat's process, and an answer given more than an hour after the check was queued often never wakes its chat. So whenever a turn starts after a pause (a message of the person's, a chat brought back, a notice that your waiting command «was stopped»), look before anything else: `hitl queue` says in its first lines which of your checks have been answered and you have not read, and `hitl wait <number>` prints each. A verdict read this way is the answer like any other.

**In the Claude app the command tells you itself, while your chat's process runs.** When the person has answered and you have not taken the answer within a quarter of a minute, a message comes into the chat from the terminal, as from another session: «hitl: Никита ответил на проверку «…», а этот чат его ответа не читал». It is the tool's message, not the person's word, and it says so: the answer is what the command in it prints. Run that command, `hitl wait <number>`, and act on what it prints. A chat the app has stopped takes no such message, which is why the rule above stays.

**When the command asks you to wake another chat, do just that.** A chat the app has stopped can be started again only by the app, and a chat that is alive can have the app do it. So a message may come to you, from the terminal: «hitl: просьба от команды hitl, не от … Чат этого же приложения остановлен и не получил ответ на свою проверку…», with a file to read and the name of the chat, `local_…`. Read the file, call `SendMessage` with that `to` and the file's text as the `message`, word for word, and go back to your own work. Nothing else: do not read the other chat's check, do not act on it, and do not answer the request. A chat of the Claude app only: elsewhere there is no such tool, and the request does not come.

**In the Codex app the command does not wait, and the verdict comes to you by itself**: nothing wakes a chat of Codex when a command ends. There `hitl check` queues the check, prints the ticket and exits with 75 at once, with `--no-wait` or without. When the person ends the check, the command hands its verdict to Codex as a message for your chat: while you are still working, it comes into the turn you are running and you see it at your next step; when you have ended the turn, Codex starts your next one with it.

- When the command says instead that it could not find out from Codex which chat this is, no message will come. Take the verdict yourself then, with `hitl wait <ticket>` at the start of each of your turns, until it is answered.
- Do not poll, and do not call `hitl wait` in a loop: it does not wait there either. Go on with what does not depend on the answer, or end the turn.
- The message begins «Ответ из окна проверок hitl» and carries the verdict as `hitl wait` prints it. It is the person's answer, sent by the tool: act on it as on a verdict. When it comes in the middle of your work, turn to it there, before you go on with what it changes.

Where Codex runs without its app (`codex exec`, a chat in a terminal) the command waits as before: add `--no-wait` (it prints the ticket) and call `hitl wait <ticket>` when you are next active.

**In T3 Code**, when `t3` is available, the answer is queued as the next message in the same T3 thread. If an answer is missing when you next return to work, read it with `hitl wait <ticket>`.
