# ConveyorCheck

The checklist of a hand check on a shared iPhone, for an iOS app.

A host, the tool on the Mac that installs builds on the phone, puts one build there at a time for a person to check by hand. It passes the app a checklist with the launch, and waits for a report file in the app's container. This package is the app's side of that: it reads the checklist, shows it in a panel at the top of the app's screen, and writes the tester's marks where the host reads them.

## Two products

- **ConveyorCheck**, plain Foundation: the checklist as the host sends it (`DeviceCheck`), the report it reads back (`DeviceCheckReport`), what a launch brought (`DeviceCheckLaunch`), and the store that keeps the checklist and its marks between launches (`DeviceCheckStore`).
- **ConveyorCheckUI**, UIKit: the panel (`ChecklistPanelView`, `ChecklistPanelController`, `ChecklistPanelWindow`), the checklist of a hand check as the panel shows it (`DeviceCheckChecklist`), and `DeviceCheckPanel`, which puts it over the app and takes it away.

The panel shows whatever a `ChecklistSource` gives it, so an app can walk a checklist of its own in the same panel.

The panel is a black band where the status bar was, which it hides for as long as it is shown. On a phone with a Dynamic Island it stands around the island: the way between the items and the three marks of the open one beside it, whose checklist it is over it, how far the check has come under it. Folded, it takes nothing from the app. Unfolded, it grows down with the item's text, the buttons of what the app can do for the item, and then with its comment and the buttons that end the checklist or show all of its items. Where there is no island the band is the same, without the island in its middle; on a screen wider than a phone it hangs from the middle of the top edge.

The band covers nothing of the app at any height. Where it goes from edge to edge and reaches below the status bar's place, the app's window starts where the band ends and is that much shorter, so the app's content, its sheets, its cards and its keyboard all stand under the band; the window is whole again when the band folds or goes away. A band that hangs in the middle of a wide screen leaves the app's window be.

A window that starts under the band has nothing above it to keep clear of, but the system still gives the status bar's inset to a view controller whose view fills the window. The package takes that inset back, by the controller's `additionalSafeAreaInsets`, from the window's root and from every screen presented over the whole of it, for as long as the window is moved, and gives it back after. Taken back a moment late, the inset is an empty strip under the band; nothing of the app is ever behind the band.

So while a check is on, the app's window may not start at the screen's corner. What the system gives in the screen's coordinates, the keyboard's frame above all, has to be converted from the screen's coordinate space (`view.convert(frame, from: screen.coordinateSpace)`), not taken as the window's own (`from: nil`): an app that did so laid its card out behind the keyboard under the unfolded band.

While the app acts for an item, the item's buttons take no taps; they dim only when the action lasts, so a press of an action that is done in a moment does not make them blink.

An item's buttons stand in lines, as many to a line as fit. There are four kinds of them, and the app says once which of its actions is which (`DeviceCheckControl`, given to `DeviceCheckPanel.show` as `controls`):

- **A link** opens a screen. Its button shows the screen's address without the app's scheme, «/screen/settings». So does the button of a deep link an item carries.
- **A switch** is two actions that undo each other, on one button: an SF Symbol or a caption for what is switched, or both, and a switch that shows how it stands. Prefer a symbol that is plain to read over words. A tap runs the action that flips it.
- **A choice** is several actions of which one holds at a time, on one button: the options in a row, the one that holds lit. A tap on an option runs that option.
- **An action** is done once: a button with an SF Symbol and no words.

A switch or a choice is one button however many of its actions an item names. How it stands is asked of the app again and again while the check is on, a few times a second, so the button follows what the tester changes in the app by hand, and what settles a moment after an action has returned. The switch is the panel's own, small: the system's one is made for a row of a list. The screen an item opened by itself, in its preparation, stands first and in the app's accent, under its address; a press runs the item's preparation again. An action the app has said nothing of is offered as before: «Подготовить» for a preparation that has one, and the action's id on a button.

What a switch or a choice is set to by a check is for the check: how each stood before the check first touched it is kept with the checklist, and put back when the check ends, on the device or at the host, before the report says so.

## In an app

```swift
let launch = DeviceCheckLaunch(arguments: ProcessInfo.processInfo.arguments)
let store = DeviceCheckStore(defaults: .standard, launch: launch)

// Once the app's window is on screen:
panelWindow = DeviceCheckPanel.show(
    store: store, over: appWindow, style: ChecklistPanelStyle(),
    interfaceStyle: .unspecified, tint: appWindow.tintColor,
    currentStyle: { .unspecified },
    controls: [                     // what the app's actions are; without it each is a button with its id
        .link("open-settings", opens: "myapp://screen/settings"),
        .toggle("Dock unfolded", symbol: "dock.rectangle", on: "dock-unfold", off: "dock-fold", isOn: { dock.isUnfolded }),
        .choice("Feed", options: [.init("range-day", title: "Day"), .init("range-week", title: "Week")], current: { feed.rangeAction }),
        .action("sign-out", name: "Sign out", symbol: "rectangle.portrait.and.arrow.right"),
    ],
    run: { ids in nil },            // the app's own actions, by id; nil, or why it could not
    openLink: { url in false },     // a deep link of the app; false when it has none
    onFinish: { /* DeviceCheckPanel.hide(panelWindow, returningKeyTo: appWindow) */ })
```

`launch.tag` is the label the host gives every launch it makes, who the build came from; the app shows it where it likes.

## What goes between the host and the app

- `--ios-run-checklist VALUE`: the checklist, as unpadded base64url of JSON with `id`, `tag`, `title` and `items`, and, when the host says it, `project`, the name of the project the check is about, which the band shows first in its line; an item has `label` and `text`, and may have `setup` and `buttons` (ids of the app's own actions) and `link`. The host finds out that a build can show a checklist by looking for the argument's name in the app's binaries.
- `--ios-run-tag VALUE`: the label of the launch.
- `Library/Application Support/ios-run/check.json` in the app's data container: the report, with `id`, `results` (`label`, `state` as `ok`, `problem` or `skipped`, `note`, and `at`, when the mark or the comment last changed, in seconds since 1970), `current` (the item the tester is at), `updatedAt`, and `finishedAt` once the tester has ended the check. It also says what the panel offers to do for the items, `offers`: for an item's `label` its `actions`, each with the `id` the host names to ask for it and its `title` in words (an address for a link); a button that is not one of plain words also says its `kind` (`link`, `toggle`, `choice` or `action`), the `symbol` the panel shows on it, for a switch how it stands (`on`, and `id` is the action that flips it), and for a choice its `options` (`id` and `title` each), the one that holds (`current`) and, as `id`, the one after it; and `ran`, the last action the host asked for that is done: `at`, the moment of the asking as the host stamped it, and `failure`, why the app could not, when it could not. The host reads it off the device.
- `Library/Application Support/ios-run/host.json`, beside the report: what the host says while the check is on the device. A host may show the same checklist on a screen of its own; what the tester marks and writes there comes in this file, as `id` and `results` (`label`, `state`, `note`, `at`), with `finished` set once the check was ended there. It may also say which item the tester is at there (`current`, `currentAt`), and that one of the buttons the report offers was pressed there: `run`, with the item's `label`, the action's `id` as `action` and the moment of the press, `at`. The app runs it as a tap on its own button, once for a press; a press the file already held when the app was launched is not run. The store reads it twenty times a second (`DeviceCheckStore.listen()`, which `DeviceCheckPanel.show` starts): of two marks of one item the later one stays, and a check ended at the host ends here and takes the panel away.
- The app's output: every time the report changes the store also says it there as one line, the byte `0x1E`, `ios-run-check`, a space and the JSON of the report file (`DeviceCheckLine.report`). A host that launched the app attached to its output has a mark the moment it is made, without reading the file; an output nobody reads costs nothing. The lines are written from a thread of their own, and the store has the app ignore `SIGPIPE`, so that an output whose reader went away neither holds the app up nor ends it.

The tester ends a check with «Готово» once every item has a mark, or with «Доработать» before that. Either sets `finishedAt`, which frees the device for the next build.

## Tests

`swift test` runs the tests of the exchange with the host and of the store on a Mac. The panel needs an app's window and scene, so its tests live with an app that uses it.
