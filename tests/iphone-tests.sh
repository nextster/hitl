#!/usr/bin/env bash
# Checks of `hitl iphone` and of the gate together with the plain ios-run.sh
# they stand on, with a made-up phone, Xcode and queue. Nothing here touches a
# device, a real chat or the real queue.
set -euo pipefail

fake_xcodebuild() {
  local argument last=""

  for argument in "$@"; do
    case "$argument" in
      -list)
        printf '%s\n' '{"project":{"schemes":["Demo"]}}'
        return
        ;;
      -showBuildSettings)
        jq -cn --arg products "$FAKE_PRODUCTS" '[{target: "Demo", buildSettings: {
          PRODUCT_TYPE: "com.apple.product-type.application", FULL_PRODUCT_NAME: "Demo.app", PRODUCT_NAME: "Demo",
          TARGET_BUILD_DIR: $products, PRODUCT_BUNDLE_IDENTIFIER: "dev.example.demo"}}]'
        return
        ;;
    esac
    last="$argument"
  done

  case "$last" in
    build)
      printf '%s\n' 'Build description path: /tmp/fake' '** BUILD SUCCEEDED ** [1.000 sec]'
      ;;
    test|test-without-building)
      printf '%s\n' 'Build description path: /tmp/fake' \
        'Executed 30 tests, with 0 failures (0 unexpected) in 0.153 (0.183) seconds' \
        '** TEST SUCCEEDED ** [12.000 sec]'
      ;;
    *)
      printf 'Unexpected fake xcodebuild invocation:' >&2
      printf ' %q' "$@" >&2
      printf '\n' >&2
      return 64
      ;;
  esac
}

fake_xcrun() {
  local output_file="" previous="" argument

  for argument in "$@"; do
    [[ "$previous" != "--json-output" ]] || output_file="$argument"
    previous="$argument"
  done

  case "${1:-} ${2:-} ${3:-} ${4:-}" in
    "devicectl list devices "*)
      printf '%s\n' '{"info":{"outcome":"success"},"result":{"devices":[{"identifier":"CORE-IPHONE","connectionProperties":{"pairingState":"paired"},"deviceProperties":{"bootState":"booted","name":"Test iPhone"},"hardwareProperties":{"platform":"iOS","udid":"UDID-IPHONE","marketingName":"iPhone Test"}}]}}' > "$output_file"
      ;;
    "devicectl device install app"|"devicectl device process launch")
      # What reached the phone, for the checks to read.
      printf '%s\n' "$*" >> "$FAKE_PHONE_LOG"
      ;;
    *)
      printf 'Unexpected fake xcrun invocation:' >&2
      printf ' %q' "$@" >&2
      printf '\n' >&2
      return 64
      ;;
  esac
}

# This file is also the made-up xcodebuild and xcrun: see the stand-ins below.
case "${FAKE_TOOL:-}" in
  xcodebuild)
    fake_xcodebuild "$@"
    exit
    ;;
  xcrun)
    fake_xcrun "$@"
    exit
    ;;
esac

TESTS_DIR="$(cd "$(dirname "$0")" && pwd -P)"
PROJECT_ROOT="$(cd "$TESTS_DIR/.." && pwd -P)"
# The plain builder: beside the commands (a working copy of both trees), or
# the one the environment names, or where it lives. The copy comes first:
# IOS_RUN_COMMAND is set for every shell on the reviewer's Mac, to the live one.
IOS_RUN="$PROJECT_ROOT/ios-run.sh"
[[ -f "$IOS_RUN" ]] || IOS_RUN="${IOS_RUN_COMMAND:-}"
[[ -f "$IOS_RUN" ]] || IOS_RUN="$HOME/Code/scripts/ios-run.sh"
[[ -f "$IOS_RUN" ]] || { printf 'ios-run.sh was not found: name it with IOS_RUN_COMMAND.\n' >&2; exit 1; }

TMP_BASE="${TMPDIR:-/tmp}"
ROOT="$(mktemp -d "$TMP_BASE/iphone-tests.XXXXXX")"
ROOT="$(cd "$ROOT" && pwd -P)"
# The commands as chats call them: links in one folder, beside the builder.
SCRIPTS="$ROOT/scripts"
PROJECT="$ROOT/Demo Project/Demo.xcodeproj"
STATE="$ROOT/conveyor-home"
export FAKE_PRODUCTS="$ROOT/products"
export FAKE_PHONE_LOG="$ROOT/phone.log"
CASE_OUTPUT=""
CASE_EXIT=0
TEST_COUNT=0

cleanup_tests() {
  case "$ROOT" in
    */iphone-tests.*) rm -rf -- "$ROOT" ;;
    *) printf 'Refusing to remove unexpected test directory: %s\n' "$ROOT" >&2 ;;
  esac
}

trap cleanup_tests EXIT

mkdir -p "$ROOT/bin" "$ROOT/home" "$SCRIPTS" "$PROJECT" "$FAKE_PRODUCTS/Demo.app"
printf '%s\n' '// fake project' > "$PROJECT/project.pbxproj"
printf 'not a binary\n' > "$FAKE_PRODUCTS/Demo.app/Demo"
: > "$FAKE_PHONE_LOG"
for tool in xcodebuild xcrun; do
  printf '%s\n' '#!/usr/bin/env bash' "FAKE_TOOL=$tool exec bash $(printf '%q' "$TESTS_DIR/iphone-tests.sh") \"\$@\"" > "$ROOT/bin/$tool"
  chmod +x "$ROOT/bin/$tool"
done
ln -s "$IOS_RUN" "$SCRIPTS/ios-run.sh"
ln -s "$PROJECT_ROOT/hitl" "$SCRIPTS/hitl"
# ios-run.sh looks for its gate under this name: the same command answers as the gate.
ln -s "$PROJECT_ROOT/hitl" "$SCRIPTS/ios-run-gate"

# run_case COMMAND ARGUMENTS...: one call on the made-up project, as the chat named by
# CHAT: "ios-run.sh" is the builder, "iphone" is `hitl iphone`, anything else is
# that command of hitl (queue, wait, drop).
run_case() {
  local command="$1"
  local -a call
  shift
  case "$command" in
    ios-run.sh) call=("$SCRIPTS/ios-run.sh" "$PROJECT") ;;
    iphone) call=("$SCRIPTS/hitl" iphone "$PROJECT") ;;
    *) call=("$SCRIPTS/hitl" "$command") ;;
  esac

  set +e
  CASE_OUTPUT="$(
    env -u IOS_RUN_COMMAND -u IOS_RUN_GATE -u IOS_RUN_TAG_PREFIX -u CONVEYOR_PROBE -u CONVEYOR_FORCE \
      HOME="$ROOT/home" \
      PATH="$ROOT/bin:$PATH" \
      CONVEYOR_CHAT="${CHAT:-}" \
      CONVEYOR_HOME="$STATE" \
      CONVEYOR_WINDOW="" \
      CONVEYOR_NO_DISPATCHER=1 \
      CONVEYOR_NOTIFY=0 \
      CONVEYOR_MAC_TEST_SCAN=0 \
      "${call[@]}" "$@" 2>&1
  )"
  CASE_EXIT=$?
  set -e
}

fail_test() {
  printf 'FAIL: %s\n' "$1" >&2
  printf '%s\n' "$CASE_OUTPUT" >&2
  exit 1
}

assert_exit() {
  [[ "$CASE_EXIT" -eq "$1" ]] || fail_test "$2: expected exit $1, got $CASE_EXIT"
}

assert_contains() {
  [[ "$CASE_OUTPUT" == *"$1"* ]] || fail_test "$2: missing '$1'"
}

assert_not_contains() {
  [[ "$CASE_OUTPUT" != *"$1"* ]] || fail_test "$2: unexpectedly contained '$1'"
}

assert_phone() {
  [[ "$(cat "$FAKE_PHONE_LOG")" == "$1" ]] || fail_test "$2: the phone saw: $(cat "$FAKE_PHONE_LOG")"
}

pass_test() {
  TEST_COUNT=$((TEST_COUNT + 1))
  printf 'ok %d - %s\n' "$TEST_COUNT" "$1"
}

run_case queue
assert_exit 0 "empty queue"
assert_contains "Телефон: проверок на него ещё не ставили." "empty queue"
pass_test "before the first check there is no queue to show"

run_case iphone --device-id CORE-IPHONE --dry-run
assert_exit 2 "a build for the phone without a mode"
assert_contains "в одном из двух режимов" "a build for the phone without a mode"
assert_contains "hitl iphone … --check" "a build for the phone without a mode"
assert_contains "hitl iphone … --probe" "a build for the phone without a mode"
assert_not_contains "xcodebuild -project" "a build for the phone without a mode"
run_case ios-run.sh --device-id CORE-IPHONE --dry-run
assert_exit 2 "ios-run.sh on the phone around the queue"
assert_contains "командой hitl iphone, в одном из двух режимов" "ios-run.sh on the phone around the queue"
assert_not_contains "xcodebuild -project" "ios-run.sh on the phone around the queue"
run_case ios-run.sh --device-id CORE-IPHONE --bundle-id dev.example.demo --launch-only
assert_exit 2 "ios-run.sh launching on the phone around the queue"
run_case ios-run.sh --device-id CORE-IPHONE
assert_exit 2 "ios-run.sh installing on the phone around the queue"
assert_phone "" "ios-run.sh on the phone around the queue"
pass_test "a build for the shared phone has to be a check or a probe, and ios-run.sh does not go around that"

CHAT="r/38 · Бары внизу" run_case iphone --device-id CORE-IPHONE --probe --dry-run -- --flag
assert_exit 0 "probe"
assert_contains "devicectl device install app --device UDID-IPHONE" "probe"
assert_contains "--terminate-existing dev.example.demo --ios-run-tag r/38 --flag" "probe"
assert_not_contains "hitl iphone, to the queue: check" "probe"
CHAT="r/38 · Бары внизу" run_case iphone --device-id CORE-IPHONE --probe -- --flag
assert_exit 0 "a probe on a free phone"
assert_contains "Проба: телефон твой" "a probe on a free phone"
assert_phone "devicectl device install app --device UDID-IPHONE $FAKE_PRODUCTS/Demo.app
devicectl device process launch --device UDID-IPHONE --terminate-existing dev.example.demo --ios-run-tag r/38 --flag" "a probe on a free phone"
[[ "$(jq -r '.probe' "$STATE/iphone/UDID-IPHONE/state.json")" == "null" ]] || fail_test "a probe on a free phone: the phone was not given back"
pass_test "a probe installs and launches itself, tagged with the chat, and gives the phone back"

CHAT="r/38 · Бары внизу" run_case iphone --device-id CORE-IPHONE --check-title "Dock bars" \
  --check "1. Swipe between screens" --check "2. Chevrons fade" --url demo://debug --dry-run -- --flag
assert_exit 0 "check"
assert_contains "-skipPackageUpdates build" "check"
assert_contains "hitl iphone, to the queue: check --udid UDID-IPHONE '--name=Test iPhone' --app $FAKE_PRODUCTS/Demo.app --bundle-id dev.example.demo --branch= --project" "check"
assert_contains "--launch-tag=r/38 --core-id CORE-IPHONE --tag=r/38 '--title=Dock bars'" "check"
assert_contains "--url=demo://debug '--item=1. Swipe between screens' '--item=2. Chevrons fade' -- --flag" "check"
assert_contains "Queueing Demo.app for a hand check" "check"
assert_not_contains "devicectl device install" "check"
assert_not_contains "devicectl device process launch" "check"
pass_test "a check hands the build and its checklist to the queue instead of installing"

run_case iphone --device-id CORE-IPHONE --check "1. Open settings" --check-setup open-settings,dock-fold --check-button dock-unfold \
  --check "2. Open the main screen" --check-link demo://screen/main --dry-run
assert_exit 0 "check with actions"
assert_contains $'\'--item=1. Open settings\037setup=open-settings,dock-fold\037buttons=dock-unfold\'' "check with actions"
assert_contains $'\'--item=2. Open the main screen\037link=demo://screen/main\'' "check with actions"
run_case iphone --device-id CORE-IPHONE --check-setup open-settings --check "1. Too late" --dry-run
assert_exit 1 "an action before its item"
assert_contains "belongs to the --check before it" "an action before its item"
pass_test "an item of a check carries the actions and the link the app offers with it"

run_case iphone --device-id CORE-IPHONE --check "1. Open [the task](https://example.test/task/320)" --dry-run
assert_exit 1 "a link written into an item of the phone's checklist"
assert_contains "shows an item's text as it is" "a link written into an item of the phone's checklist"
pass_test "a link written into a text is for the window's checks: the checklist on the phone shows a text as it is"

run_case iphone --device-id CORE-IPHONE --check "1. Hotfix" --force --dry-run
assert_exit 0 "urgent check"
assert_contains "--force '--item=1. Hotfix'" "urgent check"
run_case iphone --mac --force --dry-run
assert_exit 1 "force without the phone"
assert_contains "--force takes the physical device out of turn" "force without the phone"
pass_test "--force reaches the queue for an urgent run and means nothing off the phone"

run_case iphone --sim "Test Phone" --check "1. Что-то" --dry-run
assert_exit 1 "check on a Simulator"
assert_contains "are for a physical device" "check on a Simulator"
run_case iphone --device-id CORE-IPHONE --check "1. Что-то" --probe --dry-run
assert_exit 1 "check with probe"
assert_contains "pick one" "check with probe"
run_case iphone --device-id CORE-IPHONE --check "1. Что-то" --launch-only --dry-run
assert_exit 1 "check with launch-only"
assert_contains "does not combine with --test, --launch-only or --console" "check with launch-only"
run_case iphone --device-id CORE-IPHONE --check-title "Тема" --probe --dry-run
assert_exit 1 "title without items"
assert_contains "add --check or --checklist" "title without items"
run_case iphone --device-id CORE-IPHONE --checklist "$ROOT/no-such-file" --dry-run
assert_exit 1 "a checklist that is not there"
assert_contains "Checklist file not found" "a checklist that is not there"
run_case iphone --queue --note "слово"
assert_exit 1 "a note without its command"
assert_contains "--note goes with --queue-release or --queue-drop" "a note without its command"
pass_test "a check is a fresh build for a physical device and nothing else"

: > "$FAKE_PHONE_LOG"
CHAT="r/38 · Бары внизу" run_case iphone --device-id CORE-IPHONE --check-title "Бары внизу" --check "1. Свайпни" --no-wait
assert_exit 0 "a queued check"
assert_contains "Проверка r/38 «Бары внизу» (1 пункт) принята" "a queued check"
assert_contains "Итог: hitl wait" "a queued check"
assert_phone "" "a queued check"
[[ -f "$(jq -r '.queue[0].app' "$STATE/iphone/UDID-IPHONE/state.json")/Demo" ]] || fail_test "a queued check: the queue has no copy of the build"
[[ "$(jq -c '.queue[0].appArguments' "$STATE/iphone/UDID-IPHONE/state.json")" == '["--ios-run-tag","r/38"]' ]] || fail_test "a queued check: the launch arguments"
run_case queue
assert_exit 0 "queue status"
assert_contains "1. r/38 «Бары внизу» · 1 пункт" "queue status"
assert_contains "Убрать из очереди:         hitl drop r/NN" "queue status"
run_case ios-run.sh --device-id CORE-IPHONE --test-without-building --no-wait
assert_exit 75 "device tests behind a queued check"
assert_contains "Телефон занят, проба не запущена. В очереди: r/38." "device tests behind a queued check"
assert_not_contains "Tests complete" "device tests behind a queued check"
run_case iphone --device-id CORE-IPHONE --test-without-building --no-wait
assert_exit 75 "device tests behind a queued check, through hitl iphone"
assert_not_contains "Tests complete" "device tests behind a queued check, through hitl iphone"
CHAT="r/40 · Срочно" run_case iphone --device-id CORE-IPHONE --probe --force
assert_exit 0 "an urgent probe"
assert_contains "Срочная проба: телефон твой сейчас, в обход очереди." "an urgent probe"
run_case drop r/38
assert_exit 0 "queue drop"
assert_contains "r/38 «Бары внизу» убрана из очереди." "queue drop"
run_case ios-run.sh --device-id CORE-IPHONE --test-without-building --no-wait
assert_exit 0 "device tests on a free phone"
assert_contains "Проба: телефон твой" "device tests on a free phone"
assert_contains "Tests complete" "device tests on a free phone"
[[ "$(jq -r '.probe' "$STATE/iphone/UDID-IPHONE/state.json")" == "null" ]] || fail_test "device tests on a free phone: the probe was not released"
pass_test "a check waits in the queue with its build, and device tests wait for hand checks and give the phone back"

MAC_STATE="$STATE/turns/mac-tests/state.json"
CHAT="r/38 · Бары внизу" run_case ios-run.sh --mac --only-testing DemoTests/LoginTests
assert_exit 0 "Mac tests on a free Mac"
assert_contains "Tests complete" "Mac tests on a free Mac"
[[ "$(jq -c '[.running, .waiting]' "$MAC_STATE")" == "[null,[]]" ]] || fail_test "Mac tests on a free Mac: the Mac was not given back"
# Another checkout's run holds the Mac: this shell stands in for it.
jq -n --argjson pid "$$" --argjson since "$(date +%s)" '{running: {pid: $pid, tag: "r/14", what: "весь план", since: $since}, waiting: []}' > "$MAC_STATE"
run_case ios-run.sh --mac --test --no-wait
assert_exit 75 "Mac tests behind another run"
assert_contains "Мак занят другим прогоном тестов, твой не запущен. Сейчас: r/14 · весь план" "Mac tests behind another run"
assert_not_contains "Tests complete" "Mac tests behind another run"
run_case ios-run.sh --mac --dry-run
assert_exit 0 "Mac build beside a test run"
assert_contains "nothing is installed or launched" "Mac build beside a test run"
rm -f "$MAC_STATE"
pass_test "test runs on the Mac take turns through ios-run.sh, and a build beside them does not wait"

# What chats typed until 2026-10-09 still works: ios-run.sh hands it over.
CHAT="r/38 · Бары внизу" run_case ios-run.sh --device-id CORE-IPHONE --check "1. Swipe" --dry-run
assert_exit 0 "the old way to queue a check"
assert_contains "--check — это теперь hitl iphone" "the old way to queue a check"
assert_contains "$SCRIPTS/hitl iphone с теми же параметрами" "the old way to queue a check"
assert_contains "--tag=r/38 '--item=1. Swipe' --" "the old way to queue a check"
run_case ios-run.sh --queue
assert_exit 0 "the old way to see the queue"
assert_contains "На телефоне: проверок нет, телефон свободен" "the old way to see the queue"
pass_test "ios-run.sh --check and --queue still work: the call goes to hitl iphone and names it"

# A checkout under git: the check says which commit it was built from.
git -C "$ROOT/Demo Project" init --quiet
git -C "$ROOT/Demo Project" -c user.name=Test -c user.email=test@example.com commit --quiet --allow-empty -m "Start"
COMMIT="$(git -C "$ROOT/Demo Project" rev-parse HEAD)"
BRANCH="$(git -C "$ROOT/Demo Project" branch --show-current)"
CHAT="r/38 · Бары внизу" run_case iphone --device-id CORE-IPHONE --check "1. Swipe" --dry-run
assert_exit 0 "check from a git checkout"
assert_contains "--branch=$BRANCH --project" "check from a git checkout"
assert_contains "'--launch-tag=r/38 · $BRANCH'" "check from a git checkout"
assert_contains "--commit $COMMIT '--item=1. Swipe'" "check from a git checkout"
rm -rf "$ROOT/Demo Project/.git"
pass_test "a check carries the branch and the commit it was built from"

printf '1..%d\n' "$TEST_COUNT"
