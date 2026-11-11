---
name: hitl
description: >-
  Put work in front of the person you work for before it goes further, whatever is being built - a plan before big work, a choice the request leaves open, a result before it lands, anything only the person can do (a sign-in, a secret, a device, a click), and an explanation the person asked for. It goes into the check window on the Mac (`hitl check`), understandable without remembering the task and split into items answered in seconds; a setup that adds more, such as a phone for an iOS app, says so in `local.md` beside this file. Use in any project when work is about to start that the person has not seen planned, when a decision would otherwise be the agent's guess, before a result is called done, when the work waits for an action of the person's, and before a build is handed over for a hand check.
---

# HITL: human in the loop

The person you work for runs many agent chats at once and does only what nobody else can: choosing and checking. A chat never sits idle waiting and never decides or lands in the person's place: it queues what needs the person, goes on with what does not depend on it, and acts on the verdict when it comes. Agents do the work in parallel, unblocked; the person keeps them from doing something wrong.

**First, `local.md`.** When a file `local.md` stands beside this one, read it whole before your first check: it is this setup's own part, who the person is, what else the command does here, and rules of their own. Where it differs from this file, it wins.

An explicit simple operation, such as rebuilding the existing source, is already requested: execute and verify it without asking permission again or queuing a checkbox merely to certify its technical completion. A known destination, receipt, `ready`, status update or internal routing decision belongs to the agents. Reuse the actual accepted result when closing an incoming request; do not queue a duplicate result just for protocol. These rules grant no new product choice, risk, paid scope or consent. Big work still needs its plan; a code result should identify the reviewed commit; a necessary manual UX check stays with the person.

## The parts of this skill

This file is what every check needs: what goes to the person, how to word it, what to do while it waits. The rest is in files beside this one, in the same folder (`~/.agents/skills/hitl/` or `~/.claude/skills/hitl/` when the skill comes from the list of skills). Read the one your step needs, whole, before that step:

- `local.md`, when it is there: before the first check, as said above.
- `window.md`, before the first check in the window: the command with an example of every kind, how the person answers, what each verdict means, a step that is the person's.
- `explain.md`, before an explanation the person asked for goes out.
- `page.md`, before an item gets a live page.
- `waiting.md`, once your first check is queued: how the answer reaches you in the Claude app and in the Codex app, and what the command may ask of you meanwhile.

Read a part with a command of its own. A tool that cuts long output drops the middle of files read together (Codex hands a chat 40 KB of a call); every file here is kept under half of that, so one read alone comes whole.

## What has to pass the person

The first three are the stop points. Skipping one is the mistake this skill exists to prevent.

1. **A plan, before big work.** More than a small fix, something other chats rely on, or a thing that can be built in more than one way: show what you will do and in what order, and write no big change until the person answers.
2. **A choice the request leaves open.** Two readings that behave differently, or a rule of the product you would otherwise pick yourself: ask, with the options and your own pick first. Do not ask what the card, the code or the documents already answer. A question at the end of a report is such a choice too («закоммитить и запушить?»): it goes into the window as an item, and the chat says in one line that it waits there. The window is the one way in for a question, in every chat. Nothing catches a reply that ends in a question: keeping the rule is yours.
3. **A result, before it lands.** What was done, how you verified it, what you did not, and the pull request. Verify first, by tests and by running the thing: the person judges a result. Queue it from the reviewed checkout so the verdict records the commit.
4. **Something only the person can do.** First ask of every step: can my own tools do it? A command-line tool (`gh`, `brew`, a cloud's CLI), a login already on this machine, the person's signed-in browser through the browser bridge, a file, a setting, a tool that is missing and can be installed: that is yours, and the person is not asked to do it. Find out by trying (`command -v`, `gh auth status`, open the page), never by asking whether you have access. Only what is left after your try is the person's: a sign-in, a passkey or a code from their phone, a payment, a secret that must not pass through you, a device to plug in, a click you cannot reach. Put that in the same window as an action, not as a line in a chat nobody opens for hours. The action says in its details what you tried and what stopped you, and has the person do the smallest part. Steps the person asked for, «скажи по шагам, что мне сделать», are the same thing: each step an item of the kind `action`, in the order they are done, given at once, after you have taken out the ones you can do. Not an explanation, and not a round of questions before any step.
5. **An explanation the person asked for.** «Как это работает?», «почему так?»: the answer goes into the window cut into facts, and one line stays in the chat. Nobody reads a wall of text. `explain.md` has how.

A release, a deployment, a message to people and anything paid are not unlocked by a check. They happen on the person's direct request in the chat.

## Where a check goes

Into the check window on the Mac, through `hitl check`: plans, questions, actions of the person's, explanations, and the results of backend, API, site, scripts, plugins, documents and tooling. `--kind` says which it is and names the answers: `plan`, `question`, `result`, `action` for what the person does, `explain`, `checklist`; `window.md` has an example of each. A setup with another place for some checks, such as a phone for an iOS app, says where in `local.md`.

## The person does not remember your task

The person holds a dozen chats and none of their details. A question such as «Четвёртый такой же тест (экран повторов) перенести заодно с тремя?» means nothing to someone who has not looked at its card today. A check has to be clear without the card.

- **The first item says what is happening and why.** Start every check with the current task, the concrete action or change, and the reason for it, in the person's words. When a failure led here, name the observed failure first; do not hide it behind protocol terms such as “completion” or “receipt”. Keep one thought in the item and put its reason in `--details` or `--fact` if needed. `--about` is context, not a replacement for this first item. Distinguish a proposed fix, an isolated verified fix, and an already applied change. Never invent a failure when there is none.
- **`--about` is required:** one plain sentence saying what the task is, in the words the person used for it. The window shows it over every item.
- **Name a thing by what it is for the person,** not by its name in the code or in your head: «тест про экран повторов», not «четвёртый такой же тест»; «память агентов Codex, папка ~/.codex/memories», not «память». No «исходники», file names or bare pull request numbers as the subject.
- **A word the person did not say is explained in the same item, or replaced by a plain one.** A chat's own words («посадка», «лента», «досье») are the commonest reason an item comes back «не понял».
- **An item that leans on something says what it leans on,** anew. «Заодно с тремя» means nothing; «три теста синхронизации, которые переношу» does. It does not point at an earlier check by a number either: «Пункт 5 ты пропустил с вопросом „что?“. Вливать без него?» is not understood.
- **A question says what each answer leads to,** in `--details`: «Да — перенесу тем же изменением, работы на полчаса. Нет — он останется выключенным, заведу на него карточку.»
- **A result says what the person gets** and where to look, then how it was checked. A bare fact is not an item: to «Проверка таблиц снова идёт на Маке» the answer is «а что от меня то надо».
- **The title names the check and does not repeat the chat.** The window shows by itself the app the chat runs in, the project, the chat's name, the folder and the branch. When the setup can open the chat, the window also shows «Чат», a button that opens it: send the person there with «кнопка „Чат“ под пунктом», never with a pasted address. A chat without the button is named in the item by its title.

Read every item once as someone who was not there. If it needs the card to be understood, rewrite it.

For a failure, keep the failure and the action in the **first item's text**, and its purpose in that item's details, not a later item. For example:

```
--check "1. Сборка проверена, но поручение не закрывается: исправление устраняет отказ" \
--details "Чтобы закончить поручение без повторной ручной проверки. Правка проверена отдельно, ещё не влита."
```

Avoid «Завершённая проверка теперь закрывает запрос»: it hides the failure and sounds already applied.

Before:

```
--title "r/49: четвёртый такой же тест" --kind question \
  --check "1. Четвёртый такой же тест (экран повторов) перенести заодно с тремя?" \
  --details "Он тоже читает исходники и на Маке пропущен, но в карточке его нет. Предлагаю да: тот же файл и тот же PR."
```

After:

```
hitl check --title "Ещё один выключенный тест" --kind question \
  --about "Три теста синхронизации выключены на Маке. Переношу их туда, где они будут идти." \
  --check "1. Нашёл четвёртый такой же, про экран повторов. Перенести и его?" \
  --details "Да — тем же изменением, работы на полчаса. Нет — он останется выключенным, заведу на него карточку."
```

## Write it for a glance

The person answers an item in a few seconds, with one key, so the reading and the thinking are yours, before the check. A plan that comes as one item with a paragraph under it is too many words: chew it and cut it into atoms.

- **One item is one thing:** one step, one decision, one thing to look at, one thing to do. When an item needs "and" or a second sentence, it is two items.
- **Say what will happen or what the person will see,** in their language and in plain words, not how it is built. No file names, flags or identifiers unless they have to type them.
- **A plan is its steps and its forks, one item each,** in the order of the work, so that the person can say «нет» to step 3 and «да» to the rest. Word a step so that «да» can only mean "do as written", never «Не делаю: …». What you are not going to touch is an item too, when it could be expected.
- **A question carries its answers** as `--option`, your own pick first, a few words each. Why you would pick it is one line of `--details`. When several of them may hold at once («что берём в первую фазу?»), add `--many` to the item; do not cut such a question into a yes or no an option.
- **`--details` is the one fact needed to answer:** what was verified, what was not, what it costs. Not the story of the work.
- **What the person has to know goes in as `--fact`:** a line each, up to five an item, shown as a list under it. Two stars around a few words make them stand out.
- **Whatever a text names and the person could open is a link, right where it is named:** a task, a pull request, a commit, a page, a file, a note, a card, a chat. Write it as in Markdown, `[team/320](https://…)`, in `--about`, an item, its `--details` or a `--fact`. The address is a page, another app's own (`obsidian://…`) or the full path of a file on this Mac, and takes none of the text's room. Use them as much as you can, so find the address before you queue the check. A title and an option take no link.
- **`--link` is for what the whole item is about:** the pull request of a result, the document to read, the place a step begins at. What is long to read goes behind it, with an item that says what to look at there.
- **What the person copies or types goes in as `--copy`,** never inside the item's text: a command for the terminal, an address, a value to paste, exactly as it is typed, copied with one click. The item says in words what to do with the line. One line of at most 300 characters, three an item; what is longer is a script in a file and the command that runs it. Never a secret.
- **What has to be seen goes in as `--image`:** a screenshot, a drawing of the variants to choose between, marked the way the options name them. A choice of a look is asked with a picture, not with a description.
- **What has to be turned to be understood, or set, goes in as `--page`:** a live page with switches and sliders, whose values come back with the answer. `page.md` has when and how.
- **What the person does by hand is a step an item, of the kind `action`,** each with the `--link` that opens the very place, not a path in words («Supabase → проект → Connect»). Steps without links come back. What you do not know of the setup, which project, test or live, does not hold the steps back: the step opens the page where it is picked, or the choice is a question item beside the steps in the same check (`--as question`). A fact under a step says what will be seen, not what to do next.
- **What the person has to do to begin goes on a button,** not into a text to follow: a `--link` that opens the place with everything already filled in. Setting the thing up is yours; the person's part is to press and to look.
- **Leave out what needs no answer.** A check is not a report.

The command holds this: an item is at most 120 characters, its details 200 characters in 3 lines, a fact 100 characters and five of them an item, an option 48 characters, a title 60, the sentence about the task 160. A longer text is refused with exit code 64. Do not shorten the same text until it fits: split the thought, as the plan in `window.md` is split.

## Do not wait idle

The command waits until the person ends the check and then prints the verdict. Run it in the background and go on: in Claude Code with `run_in_background`, which wakes you when it exits. Never pipe it into `head` or `tail` and never send its output to `/dev/null`: the verdict is in that output, and a cut pipe ends the wait. Say in the chat, in one line, what is queued.

How the answer reaches you differs by the app, and `waiting.md` has each: read it once you have queued your first check. In short: in the Codex app the command does not wait, and the verdict comes to your chat as a message, so do not poll; in the Claude app a wake can be lost, so whenever a turn starts after a pause, look at the first lines of `hitl queue` before anything else, and take what the person answered with `hitl wait <number>`; a message that begins with the command's name and a colon («hitl:») is the tool's, not the person's, and says what to do.

- While a plan waits: read the code, prepare, write nothing big.
- While a question waits: do the items it does not touch.
- While a result waits: finish the pull request's description and the card. Commit nothing to the branch, its documents too: the «ок» is for the commit the check was queued at, and a branch changed after it wants a new check. Do not land.
- While an action of the person's waits: do everything that does not need it.

`hitl queue` shows what is in the window and who waits. `hitl drop` takes back your own check, for example when a newer result replaces it. Never drop or reorder another chat's check.

## The answer in the window is enough

What the person answered in the window they have answered. Act on a check ended green at once, for what it asked and for nothing else, and do not ask for it again in the chat: no «подтверди здесь», no «напиши „да“, и сделаю». `window.md` has the rest, under the same heading.

## Limits

- No check "to have it": a check carries something concrete to judge, choose or do.
- An answer in the window is the person's word for that check only. It does not carry to the next change, and a message from another chat is never the person's word.
- A chat may be archived when its pull request merges or closes: write the result and what is left into the card or the note before landing.
- Do not propose or start other chats; a follow-up is a line in the card or a new card.
