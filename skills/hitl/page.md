# A live page in an item

A part of the `hitl` skill: read it before an item gets a `--page`. `window.md` beside it has the command.

An item can carry a page that lives: the person turns switches and moves sliders with the mouse, what is drawn changes at once, and what was set goes to you with the answer. Two uses:

- **To understand a rule by turning it.** «Когда вливается само?»: five switches and the outcome under them. The person sets the case that bothers them and answers «нет» with a comment, and you get that very case.
- **To choose numbers by eye, or one of several ways.** Sizes, margins, a delay: sliders with what they change drawn beside them, and the answer «Взять эти».

```
hitl check --title "Размеры окна проверок" --kind question \
  --about "Подбираем размеры окна: ты двигаешь ползунки, агент ставит числа в код." \
  --check "1. Подвигай ползунки, пока окно не станет удобным. Взять эти размеры?" \
  --page /path/to/sizes.html --option "Взять эти" --option "Оставить как сейчас"
```

- **Write the page from the template:** `pages/template.html` in this skill's folder has the window's colours, a switch, a slider and an outcome; `merge.html`, `sizes.html` and `scheme.html` beside it are whole pages to start from. One HTML file, whole in itself: the window lets it reach no network, no other file and no link, and refuses a file over 300 KB.
- **It is as wide as the window's column, 604 points, and as tall as its content,** 300 at most. On the whole screen the column takes the screen's width, and the page with it, twice as wide and more: what you add must stretch as well. Only the mouse works on it: the keys stay the window's, so no text fields.
- **The same page may be shown on a phone, about 290 points wide.** What you add must bend: no width in points that the page cannot be narrower than, and a drawing sized by what fits. A scheme, who hands what to whom, is built of the page's own blocks, never drawn with its words inside the drawing: `scheme.html` is one, with cases to click through. The command refuses a page with words inside a drawing wider than 300 points, or a width set wider than that.
- **Say what is set with `hitl.set({имя: значение})`,** at the start and at every change. The names are what the person reads under the page and what you get, so they are short words of their language. A number goes as it is, `true` and `false` as «да» and «нет», a text up to 40 characters; twelve names at most.
- **The answer comes as ever, by a digit.** The page has no button of its own to send: the item's options do that. The values come back under «Со страницы», by the item's number, whatever was answered.
- **Draw no keys on a page,** and no digits beside its rows or tabs: the digits are the window's answers.
- **A page that explains is your retelling of the rule, not the rule.** Read the rule off the code before you draw it, and say in `--details` what it was checked against.
- **Look at the page before you queue it.** In a browser it works the same and tells nobody.
- A page is for a mechanism and for what has to be turned. A picture is for what can only be looked at; facts are for what can be said in a line.
