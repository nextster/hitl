# An explanation the person asked for

A part of the `hitl` skill: read it before an explanation goes out. `SKILL.md` beside it has how to word an item; `window.md` has the command.

The person asks how something works or where something comes from, and the honest answer is five paragraphs. Nobody will read them. The reading and the cutting are yours:

```
hitl check --title "Откуда уведомление «Влито в main»" --kind explain \
  --about "Ты спросил, кто шлёт уведомление «Влито в main» и нужно ли оно." \
  --check "1. Уведомление шлёт сам скрипт вливания, в конце каждого вливания" \
  --fact "Это системное уведомление, не terminal-notifier: поэтому «Показать» открывает Script Editor" \
  --fact "То же самое уже видно в **очереди проверок**, в отчёте чата и на GitHub" \
  --check "2. Убрать уведомление об успешном вливании?" \
  --fact "Выключателя для него одного нет: только все уведомления разом" \
  --option "Только «Влито», а «Не влито» оставить" --option "Убрать оба" --option "Оставить как есть"
```

- **Begin with the present task and its cause**, as the core skill requires. If a failure caused the work, the first item names what failed, what the named chat is doing about it, and why. When explaining how a request led to a new task, keep the path visible: the person's original request, the observed failure, the accepted follow-up, and the current action. Separate verified facts from a proposed or unapplied fix.
- **The item is the answer,** in one sentence: what was asked, answered. Not «Разобрался с уведомлением».
- **Steps for the person are not an explanation.** «Скажи по шагам, что мне сделать» is answered with items of the kind `action`, a step each, with the link that opens its place (`window.md`, "Something only the person can do"). An explanation is for how a thing works or where it comes from.
- **Who does what is said by name, never «я».** Several agents may stand behind that word, and «а "я" это кто?» is the answer it gets. Name the chat by its name, the agent, the command, the app. The command refuses «я», «мне», «мой» in an explanation, outside the person's own words in «…».
- **A mechanism is shown, not told, and by a live page wherever it can be one.** A picture is for what cannot be turned. A mechanism of three parts or more is never text alone: draw the parts as boxes and who hands what to whom. `page.md` has how.
- **The facts are what stands behind it,** a line each, at most five: what the person would ask next. No file names or line numbers. What they may want to open is a link in the fact itself.
- **The question it ends in is an item with its options,** with the facts needed for that choice.
- **More than fits is more items,** not a longer one. The command refuses a sixth fact and a fact over 100 characters.
- In the chat stays one line: that the answer is in the window.
- «Не понятно» comes with a comment: explain that item again as a new check, another way and shorter. Do not repeat the same text.
