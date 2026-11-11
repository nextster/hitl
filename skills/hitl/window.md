# The check window on the Mac

A part of the `hitl` skill: read it before the first check in the window. `SKILL.md` beside it says what goes to the person and how to word it; here is the command, how the person answers, and what each verdict means.

One floating window serves every chat. It shows one check at a time, item by item, and the other checks wait their turn under it. Run `hitl check --help` once before the first use.

## The command

A result:

```
hitl check --title "Вход для плагина" --kind result \
  --about "Плагин Obsidian: делаю ему вход без пароля." \
  --check "1. Плагин входит на Development и получает токен" --details "Проверено: 14 тестов и вход руками." \
  --link https://github.com/owner/repo/pull/12 \
  --check "2. Вход с телефона не проверял: влить без него?"
```

A question, with options or as a plain yes or no:

```
hitl check --title "Репозиторий плагина" --kind question \
  --about "Плагин Obsidian: начинаю писать, нужно место для кода." \
  --check "1. Где лежит репозиторий плагина?" \
  --option "Свой репозиторий" --option "Внутри репозитория продукта" \
  --details "Предлагаю свой: так решили 07.10." \
  --check "2. Показывать баннер гостям?"
```

A plan. Before, refused, one item with a paragraph under it:

```
--kind plan --check "1. Собрать полный каталог покупок и выбрать варианты" \
  --details "Сначала пройду все чеки за год и соберу позиции в таблицу, потом для каждой найду три варианта по цене и отзывам, сравню доставку, сведу в доску. Подписки не трогаю. Где чека нет, возьму выписку банка…"
```

After, a step an item:

```
hitl check --title "Покупки: каталог" --kind plan \
  --about "Собираю каталог всего купленного за год, чтобы выбрать, что заменить." \
  --check "1. Собираю все покупки за год из чеков в одну таблицу" \
  --check "2. Где чека нет, беру строку из выписки банка" \
  --check "3. К каждой позиции ищу три варианта: по цене и по отзывам" \
  --check "4. Подписки не трогаю" \
  --check "5. Итог — доска: позиция, варианты, мой выбор" --link "$HOME/Documents/покупки.md"
```

Something only the person can do, here a command they run:

```
hitl check --title "Копия базы Production" --kind action \
  --about "Состояния задач переезжают в новую схему: перед переносом нужна копия базы Production, снять её можешь только ты." \
  --check "1. Запусти скрипт в терминале и вставь адрес базы из менеджера паролей → архив ляжет в папку" \
  --copy "~/backups/copy.sh"
```

What belongs to one matter comes as one check, whatever kinds its items are: an explanation, the question about it and the step that is the person's are three items of one check, not three checks in a row. `--as explain`, `--as question`, `--as action`, `--as plan` or `--as checklist` after a `--check` gives that item its own kind.

```
hitl check --title "Проверки GitHub стоят" --kind question \
  --about "Проверки на GitHub остановились, и ни одно изменение сейчас не влить." \
  --check "1. GitHub остановил проверки: кончились оплаченные минуты этого месяца" --as explain \
  --fact "В тариф входит 2000 минут, к 10 октября потрачено 2000" \
  --check "2. Какой потолок расходов поставить до конца месяца?" --option "20 $" --option "50 $" \
  --check "3. Открой оплату GitHub и поставь потолок → проверки пойдут снова" --as action \
  --link https://github.com/organizations/ORG/settings/billing
```

- `--title` and `--about` are for the whole check; `--details`, `--fact`, `--link`, `--copy`, `--image`, `--page`, `--option`, `--many` and `--as` belong to the `--check` before them. Up to four pictures an item; a `--link` to a picture file on this Mac is shown as a picture too. A link to a note of an Obsidian vault opens in Obsidian: give the file's full path.
- A leading `3.` or `4a.` stays the item's number; keep the numbers of the person's request.
- One check per coherent thing, with as many items as it has things. Several small questions of one task go into one check, not into five.
- `--kind` names the answers: `checklist` (работает / не работает / пропустить), `result` (ок / не ок / пропустить), `plan` (да / нет), `question` (да / нет / пропустить, or the item's own `--option`), `action` (сделал / не сделаю / позже), `explain` (понятно / не понятно).
- A result is a check of its own, with nothing else in it and no options on its items: the «ок» records approval of the reviewed result. A question with options goes into a check of its own.
- The check remembers the chat, the branch and the commit it was asked from.

## How the person answers

Write the items so that they fit this. Every answer is a digit, by the answer's place; a question's options are its digits, and after them stands «другое», an answer in the person's own words. Under the answers of every item stands «не понял», on 0: it is not a no, it asks for the item to be said again. The person can type a comment at any item and paste screenshots into it, put a long check off behind the short ones, and take any waiting check first. You do nothing for any of it.

## Something only the person can do

- Before you queue a step, do what your own tools reach (`SKILL.md`, item 4): an action that your CLI, a login on this machine or the signed-in browser could have done comes back. The step that is left says in its details what you tried and what stopped you.
- Ask for the smallest thing, say what it unlocks after the arrow, and give the link that opens the place.
- One step an item, each with the button that opens the very place. «Открой оплату в GitHub: почини платёж или подними лимит» is not a step: it has no concrete place and no concrete action. Split actions as finely as they go, so that one button brings the person to the right place.
- A command to run comes as `--copy` on the item, and the item's text says where to run it and what it does.
- Never ask for a secret itself. The person sets it where it lives; a password, a key or a token never passes through the window, a comment or a chat.
- «Сделал» is the person's word, not proof. Check with your own tool that it took effect before going on.
- Where you can watch for it yourself, keep watching, every few seconds for up to five minutes. When you see it done before the answer, take your item back with `hitl drop`, so that there is nothing to press.
- «Не сделаю»: read the comment, find another way or leave that part, and say so in the card. «Позже»: go on with what does not need it and queue it again when it is the only thing left.

## The verdict

It comes as item numbers by answer, the choices and the comments. Exit code 0 is a verdict whatever the answers; 2 means the check was dropped and nothing was looked at.

- **Every item answered yes and ended with «Готово»** is the OK for what was asked, and the verdict says so. A plan: do it. A question: go on with the choice and write it into the card in the person's words. A result: use the approval for the reviewed change.
- **«Нет», «не ок», «не работает»:** fix what the comment says and queue a new check for those items. A comment you cannot act on is a question about that item, a shorter one.
- **«Не понял»,** and «не понятно» to an explanation: the item was not understood, and that is not a «нет». Say it again as a new check, another way and shorter: what the thing is for the person, no word they did not say, an example or a picture; the comment says what was unclear. The other answers of the check stand: do not ask them again. A check with a «не понял» is not the OK for what it asked as a whole.
- **An item you ask again brings its last answer along.** Add `--after <the number of the check you are answering>` to the new check and leave the item its number: over it the window shows when and what was answered then. Without it, one question can come four times in half an hour, each time as a new one.
- **A check of several kinds** comes back item by item, each with the word of its kind and its comment: «1. объяснение: понятно», «2. вопрос: выбрал «20 $»», «3. действие: позже. «вечером»». Act on each line by its kind.
- **«Свой ответ, не из вариантов»:** none of your options fitted, and the comment is the answer. Follow it.
- **«Со страницы»:** what a live page was set to at the answer, by the item's number.
- **Screenshots:** pasted into a comment, the verdict lists their files under the item's number. Open each file before acting on the comment.
- **No mark:** the person did not look. Put the item into the next check.
- **Skipped:** the person chose not to judge it now. Leave it out and list it as unchecked in the pull request.

## The answer in the window is enough

What the person answered in the window they have answered. Do not ask for it again in the chat: no «подтверди здесь», no «напиши „да“, и сделаю», no waiting for a word there before acting on a check ended green.

- **Act on the verdict at once,** for what the check asked and for nothing else. That an action is outward or hard to undo is why it went into the window; it is not a reason to ask twice.
- **Before an action Claude Code's auto mode may stop** (outward, hard to undo), print the verdict by a command of your own turn: `hitl wait <number>`, the number in the verdict's last line. Auto mode reads the commands you run, never what they print, a file, or the output of a command that waited in the background.
- **Do not ask ahead of a refusal.** That Claude's protection stopped one action is not a reason to ask for a word on the next ones in advance: do them and see.
- **When Claude's protection stops an action approved in the window** («denied by the auto mode classifier»), the person hears of it in the window, once. Finish what the refusal does not touch, then queue one check of the kind `action` with one item: what was stopped, in the person's words, and that it waits for their word in the chat, which the button «Чат» under the item opens. Then leave that action alone.
- **In that item, and after it:** do not write the words to type, do not send the person to change the chat's mode, do not explain the protection, do not try the action another way, by another tool or in parts, and do not queue a second check about the same refusal. A chat that has no «Чат» button names itself in the item by its title.
- **The person's word in the chat, when it comes,** is for the action they were told of. Do it. If the protection stops it again, say so once, in the window as before, and leave it.

```
hitl check --title "Защита Claude остановила разбор почты" --kind action \
  --about "Разбираю входящие по твоим ответам в окне. Одно действие защита Claude не пропустила." \
  --check "1. Остановлено: убрать 19 писем в архив. Ждёт твоего слова в чате: кнопка «Чат» под пунктом"
```
