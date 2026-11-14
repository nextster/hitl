# HITL

Human-in-the-loop checks for coding agents. Ask a person for a decision in a Mac window, share a build for review on an iPhone, and return the answer to the chat that requested it.

## macOS

Use the packaged Mac window with Homebrew:

```sh
brew install nextster/tap/hitl
hitl setup --name "Your name"
```

For the Mac host with iPhone support, clone this repository and run `./hitl`. It builds the Rust host and compiles the Mac window on demand. Run `./hitl --help` for the available commands.

```sh
hitl check --kind question --title "Editor behavior" \
  --about "Choose what Enter does in the comment field." \
  --check "1. What should Enter do?" \
  --option "Send the comment" --option "Insert a new line"
```

`hitl queue` shows pending checks. `hitl wait <id>` retrieves a verdict. Codex Desktop and T3 Code can receive the verdict in the originating chat. The Mac window's **Chat** button opens supported native chat links. T3 Code 0.1.9 can receive verdicts but has no external link to open a particular desktop chat.

A result records the decision and the reviewed commit. The calling agent handles subsequent repository actions.

## iPhone

The host exposes two related workflows:

- `hitl iphone --check` queues a build with its own checklist. The Swift package under `ConveyorCheck/` lets the app display that checklist and report answers.
- `hitl phone` serves the Mac window's queue to the companion app under `demo/`. Build it with your own Apple signing configuration and use `hitl phone pair` to pair it over USB. A deployment of `relay/` is optional for access outside the local network.

For app-supported disposable sign-in, `--launch-env-file` supplies private per-ticket child environment values. See [private launch input](docs/iphone-private-launch-input.md) for the file format and isolation guarantees.

The build queue uses the executable named by `IOS_RUN_COMMAND` and the `ios-run-gate` link. The builder supplies a build-only result and uses the gate before touching shared hardware. For host setup, see `./hitl install --help`.

## State and development

State defaults to `~/.hitl` and can be set with `HITL_HOME`. On an existing Mac host, HITL can use the previous `~/.conveyor` state in place to keep pending checks, history, and phone pairing.

Run `./hitl test --all` for the Rust and scripted Mac window suites. Run `./hitl test package` for the Swift package. The `iphone` test suite also needs the local iOS builder and uses a simulated device.
