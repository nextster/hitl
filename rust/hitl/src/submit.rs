//! A check put into the queue: its options read and held to the limits, the ticket written, the window brought up,
//! the wait begun. The port of `check.parse_submission`, `check.submit`, `check.command_submit` and `check.main`.
//!
//! A chat queues what it needs from the person who answers: a look at a result, a yes to a plan, an answer to a
//! question, something only that person can do. The window shows one check at a time, the others wait their turn,
//! and the chat goes on working. The answers come back as a verdict by item number.
//!
//! He holds a dozen chats in his head and none of their details. So a check says what the task is (--about), an
//! item is read and answered in seconds, and the texts have limits: what does not fit is split into more items or
//! goes behind a link.
#![allow(dead_code)]

use crate::{common, queue, tell, verdict, window};
use serde_json::{Map, Value, json};
use std::path::{Component, Path, PathBuf};

pub const KINDS: [(&str, &str); 6] = [("checklist", "чек-лист"), ("result", "результат"), ("plan", "план"),
                                      ("question", "вопрос"), ("action", "действие"), ("explain", "объяснение")];

/// The words of a kind's answers, yes, no, not now (check.ANSWERS).
pub fn answers(kind: &str) -> &'static [&'static str] {
    match kind {
        "checklist" => &["работает", "не работает", "пропустить"],
        "result" => &["ок", "не ок", "пропустить"],
        "plan" => &["да", "нет"],
        "question" => &["да", "нет", "пропустить"],
        "action" => &["сделал", "не сделаю", "позже"],
        "explain" => &["понятно", "не понятно"],
        _ => &[],
    }
}

/// The names of the groups of a verdict, by kind (check.GROUPS).
pub fn groups_of(kind: &str) -> [&'static str; 3] {
    match kind {
        "result" => ["Ок", "Не ок", "Пропущено"],
        "plan" | "question" => ["Да", "Нет", "Пропущено"],
        "action" => ["Сделано", "Не сделано", "Позже"],
        "explain" => ["Понятно", "Не понятно", "Пропущено"],
        _ => ["Работает", "Не работает", "Пропущено"],
    }
}

/// The word a kind is shown under (check.KINDS[kind]); None for no kind.
pub fn kind_title(kind: &str) -> Option<&'static str> {
    KINDS.iter().find(|(name, _)| *name == kind).map(|(_, title)| *title)
}

// What one item may hold. He reads and answers an item in seconds; what does not fit is split into more items, or
// goes behind a link.
pub const MAX_TITLE: usize = 60;
pub const MAX_ABOUT: usize = 160;
pub const MAX_ITEM: usize = 120;
pub const MAX_DETAILS: usize = 200;
pub const MAX_DETAIL_LINES: usize = 3;
pub const MAX_FACT: usize = 100;
pub const MAX_FACTS: usize = 5;
pub const MAX_OPTION: usize = 48;
pub const MAX_OPTIONS: usize = 9;
pub const MAX_LINKS: usize = 3;
// A line he copies with a click is not read: it may be longer than anything he reads, and is shown cut.
pub const MAX_COPY: usize = 300;
pub const MAX_COPIES: usize = 3;
pub const MAX_IMAGES: usize = 4;
pub const MAX_PAGE_BYTES: u64 = 300_000;
pub const PAGES: [&str; 2] = [".html", ".htm"];
/// In the order Python's `sorted(PICTURES)` gives them.
pub const PICTURES: [&str; 9] = [".bmp", ".gif", ".heic", ".jpeg", ".jpg", ".png", ".tif", ".tiff", ".webp"];
/// How wide a live page is in the app on his phone, in the page's own points, with a little to spare.
pub const PHONE_PAGE: f64 = 300.0;

pub const ABOUT: &str = "The person does not remember your task. Say what it is with --about: one plain sentence in the person's words, like \
--about \"Три теста синхронизации выключены на Маке: переношу их туда, где они идут\". Then word every item so \
that it is clear without the card: what the thing is for the person, not its name in the code, and for a question what \
each answer leads to.";

pub const SPLIT: &str = "Split it: one item is one thing the person answers in a few seconds without opening anything. A plan becomes one \
item per step or decision, a long question becomes several short ones, and what has to be read goes \
behind --link.";


/// Ambiguous wording to flag in host checks, with suggested replacements.
/// (check.HIS_UNKNOWN_WORDS): hitl's alone, his team's words are theirs. A word comes here from
/// `hitl stats`, on his «да» to it.
const HIS_UNKNOWN_WORDS: [(fn(&[char]) -> Option<String>, &str, &str); 2] = [
    (landing_word, "влить в main", "неясно, имеется ли в виду слияние ветки"),
    (live_folder, "папка, из которой чаты запускают инструменты", "неясно, о какой папке речь"),
];

/// A sentence of the help about a button only hitl's window offers; his team's help goes without it.
const NOTE_BUTTON: &str = "A chat named\n«team/NN · …» (or the old «r/NN · …») gets a button that opens its task note in Obsidian. ";

pub const USAGE: &str = r#"Usage:
  hitl check --title TEXT --about TEXT [--kind checklist|result|plan|question|action|explain]
                 --check TEXT [--details TEXT] [--fact TEXT]... [--link URL|PATH]...
                 [--copy TEXT]... [--option TEXT]... [--many] [--image PATH]...
                 [--page PATH] [--as KIND]
                 [--check TEXT ...]
                 [--after ID] [--tag TAG] [--no-wait]

  --title    What the check is, in a few words. Not the chat's name: the window
             shows the chat by itself.
  --about    What the task is, one plain sentence in the person's words, for someone who
             does not remember it. Required; at most 160 characters. The window
             shows it over every item.
  --check    One thing, answered in a few seconds: "what to do → what should be
             seen", one step of a plan, one question, one thing for the person to do.
             At most 120 characters. A leading "3." or "4a." stays the
             item's number.
  --details  A line or two under the item above it: what the person has to know to
             answer; for a question, what each answer leads to. At most 200
             characters and 3 lines.
  --fact     A short fact under the item above it: what the person has to know, one line
             of at most 100 characters, up to 5 of them, shown as a list.
             This is how an explanation comes: the answer as the item, the
             facts under it, and the question about it, when there is one, as the
             item's own --option. **Two stars** make a few words stand out.
  --link     Something to open for the item above it: a pull request, a page,
             a file. What is long to read goes here, not into the text. Up to 3.
             A thing a text only names, a task, a pull request, a page, is a link
             in the text itself: [team/320](https://…) in --about, in a --check,
             its --details or a --fact stands as «team/320», and a click on those
             words opens the address: a page, another app's own address, the full
             path of a file on this Mac. The limits count the words that are read; the
             address takes none of the text's room.
  --copy     A line copied with one click, for the item above it: a command for
             the terminal, an address, a value to paste somewhere. The window shows
             it on a line of its own in the font of code; a click on it, or ⌘ with
             its digit, puts all of it into the clipboard. One line of at most 300
             characters, up to 3 an item. Whatever is to be copied or typed goes
             here, exactly as it is typed, and not into the item's text.
  --image    A picture for the item above it, shown right in the window: a
             screenshot, a drawing of the variants to choose between. Up to 4.
             A click, or ⌘ with its number, shows it large in the middle of the screen.
  --page     A live page for the item above it: one HTML file with switches and
             sliders turned with the mouse, shown in the window where the
             pictures stand. Whatever the page hands to `hitl.set({name: value})`
             comes back with the answer, under "Со страницы". The file is whole in
             itself: it reaches no network and no other file, and is at most
             300 KB. One page an item.
  --option   An answer to offer for the item above it: a few words, at most 48
             characters, 2 to 9 of them. Makes the item a question; the window
             adds «другое» for an answer in the person's own words.
  --many     Several of the options of the item above it may hold at once: each
             gets a box, a digit ticks it or takes the tick off, and the person goes on to
             the next item by hand. The ticked ones come back as a list.
  --kind     What the answers are called:
               checklist  работает / не работает / пропустить (the default)
               result     ок / не ок / пропустить
               plan       да / нет
               question   да / нет / пропустить, or the item's own --option
               action     сделал / не сделаю / позже: something only the person can do
               explain    понятно / не понятно: an explanation the person asked for
  --as       The kind of the item above it, where it is not the check's: explain,
             question, action, plan or checklist. So one check carries what belongs
             to one matter: the explanation, the question about it and the step that
             is the person's, each with the answers of its kind. The verdict of
             such a check comes item by item, in their order. Not in a result: an
             «ок» to a result is the word for a merge, and a result is a check of
             its own.
             Under the answers of every item the window adds «не понял», always on
             the key 0. It is not a no: the verdict lists such items apart, and the
             chat says them again, another way; the other answers of the check stand.
  --after    The number of the ended check this one answers: items of it are asked
             again here, under the same numbers. Over each such item the window
             shows what the person answered to it then, when something was written or it
             was not a yes: a question that comes again is a new one otherwise. The verdict of a check says when to use it.
  --no-wait  Queue the check and exit with 75; `hitl wait` picks it up later.

A text over its limit is refused: split it into more items. A chat named
«team/NN · …» (or the old «r/NN · …») gets a button that opens its task note in Obsidian. The command waits
until the check is ended in the window and prints the verdict; screenshots
pasted into a comment come as file paths to open. Exit codes: 0 a verdict,
whatever the answers; 2 the check was dropped; 64 a refused call; 75 queued
with --no-wait.

In a chat of the Codex app the command never waits, with --no-wait or without:
it queues the check and exits with 75, and when the check is ended its verdict
comes to the chat as a message, with which Codex starts the chat's next turn.
Nothing is to be polled there. A check queued by a helper is told to the chat
that started the helper.

What waits is shown by `hitl queue`; `hitl wait [ID|TAG]` waits for a
check again, `hitl drop [ID|TAG]` takes the caller's own check back.
"#;

/// The help as this edition says it: hitl's own command names in place of hitl's.
pub fn usage() -> String {
    if common::is_host() { USAGE.to_string() } else { hitl_words(&USAGE.replace(NOTE_BUTTON, "")) }
}

/// `re.sub(r"\bconveyor (check|queue|wait|drop)\b", r"hitl \1", text)`.
fn hitl_words(text: &str) -> String {
    let characters: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut position = 0;
    while position < characters.len() {
        let at_start = position == 0 || !is_word(characters[position - 1]);
        if at_start && starts(&characters, position, "hitl ") {
            let after = position + "hitl ".len();
            let found = ["check", "queue", "wait", "drop"].into_iter().find(|word| {
                let end = after + word.chars().count();
                starts(&characters, after, word) && (end == characters.len() || !is_word(characters[end]))
            });
            if let Some(word) = found {
                out.push_str("hitl ");
                out.push_str(word);
                position = after + word.chars().count();
                continue;
            }
        }
        out.push(characters[position]);
        position += 1;
    }
    out
}

// --------------------------------------------------------------------------
// Text, as Python's str and re read it

/// Python's str.isspace(): Rust's white space and the four separators \x1c-\x1f.
fn py_space(character: char) -> bool {
    character.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&character)
}

/// A character of `\w`.
fn is_word(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

/// `" ".join(text.split())`.
fn squeeze(text: &str) -> String {
    text.split(py_space).filter(|part| !part.is_empty()).collect::<Vec<_>>().join(" ")
}

/// `text.strip()`.
fn strip(text: &str) -> &str {
    text.trim_matches(py_space)
}

/// `text.splitlines()`.
fn splitlines(text: &str) -> Vec<&str> {
    let breaks = |character: char| matches!(character, '\n' | '\r' | '\u{0b}' | '\u{0c}' | '\u{1c}' | '\u{1d}' | '\u{1e}'
                                                  | '\u{85}' | '\u{2028}' | '\u{2029}');
    let mut lines = Vec::new();
    let mut start = 0;
    let mut characters = text.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        if breaks(character) {
            lines.push(&text[start..index]);
            let mut next = index + character.len_utf8();
            if character == '\r' && characters.peek().is_some_and(|&(_, following)| following == '\n') {
                characters.next();
                next += 1;
            }
            start = next;
        }
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

/// A character as `re.IGNORECASE` compares it: its simple lower case, and the letters sre holds equal to another.
fn fold(character: char) -> char {
    let lower = if character == 'İ' {
        'i'
    } else {
        let mut lowered = character.to_lowercase();
        match (lowered.next(), lowered.next()) {
            (Some(single), None) => single,
            _ => character,
        }
    };
    match lower {
        'ı' => 'i',
        'ſ' => 's',
        'µ' => 'μ',
        'ᲀ' => 'в',
        'ᲁ' => 'д',
        'ᲂ' => 'о',
        'ᲃ' => 'с',
        'ᲄ' | 'ᲅ' => 'т',
        'ᲆ' => 'ъ',
        'ᲇ' => 'ѣ',
        other => other,
    }
}

/// Whether `pattern` (in lower case) stands at `at`, letters compared as `re.IGNORECASE` does.
fn starts_folded(text: &[char], at: usize, pattern: &str) -> bool {
    let mut position = at;
    for wanted in pattern.chars() {
        if position >= text.len() || fold(text[position]) != fold(wanted) {
            return false;
        }
        position += 1;
    }
    true
}

/// Whether `pattern` stands at `at`, exactly.
fn starts(text: &[char], at: usize, pattern: &str) -> bool {
    let mut position = at;
    for wanted in pattern.chars() {
        if position >= text.len() || text[position] != wanted {
            return false;
        }
        position += 1;
    }
    true
}

/// Where `pattern` (in lower case) next stands at or after `from`, compared as `re.IGNORECASE` does.
fn find_folded(text: &[char], from: usize, pattern: &str) -> Option<usize> {
    (from..text.len()).find(|&at| starts_folded(text, at, pattern))
}

fn word_end(text: &[char], from: usize) -> usize {
    let mut end = from;
    while end < text.len() && is_word(text[end]) {
        end += 1;
    }
    end
}

fn spaces_end(text: &[char], from: usize) -> usize {
    let mut end = from;
    while end < text.len() && py_space(text[end]) {
        end += 1;
    }
    end
}

fn run_end(text: &[char], from: usize, member: impl Fn(char) -> bool) -> usize {
    let mut end = from;
    while end < text.len() && member(text[end]) {
        end += 1;
    }
    end
}

fn piece(text: &[char], start: usize, end: usize) -> String {
    text[start..end].iter().collect()
}

/// Where a word may begin: `\b` before a word character.
fn word_start(text: &[char], at: usize) -> bool {
    at < text.len() && is_word(text[at]) && (at == 0 || !is_word(text[at - 1]))
}

/// `\bпосадк\w*|\bпосад(?:ить|им|ит|ят|и|ил|ила|или)\b|\bсаж(?:ать|аем|ает|ают|ай|аю)\b`, ignoring case.
fn landing_word(text: &[char]) -> Option<String> {
    let bounded = |end: usize| end == text.len() || !is_word(text[end]);
    for at in 0..text.len() {
        if !word_start(text, at) {
            continue;
        }
        if starts_folded(text, at, "посадк") {
            return Some(piece(text, at, word_end(text, at + 6)));
        }
        let endings: [(&str, &[&str]); 2] = [("посад", &["ить", "им", "ит", "ят", "и", "ил", "ила", "или"]),
                                              ("саж", &["ать", "аем", "ает", "ают", "ай", "аю"])];
        for (stem, tails) in endings {
            if !starts_folded(text, at, stem) {
                continue;
            }
            let after = at + stem.chars().count();
            for tail in tails {
                let end = after + tail.chars().count();
                if starts_folded(text, after, tail) && bounded(end) {
                    return Some(piece(text, at, end));
                }
            }
        }
    }
    None
}

/// `\bжив(?:ая|ой|ую|ою)\s+папк\w*`, ignoring case.
fn live_folder(text: &[char]) -> Option<String> {
    for at in 0..text.len() {
        if !word_start(text, at) || !starts_folded(text, at, "жив") {
            continue;
        }
        for tail in ["ая", "ой", "ую", "ою"] {
            if !starts_folded(text, at + 3, tail) {
                continue;
            }
            let gap = at + 5;
            let after = spaces_end(text, gap);
            if after > gap && starts_folded(text, after, "папк") {
                return Some(piece(text, at, word_end(text, after + 4)));
            }
        }
    }
    None
}

/// «Я» in an explanation: he holds a dozen chats, and to «Я это узнал, а защита нет» he said «а "я" это кто»
/// (check.FIRST_PERSON, ignoring case).
fn first_person(text: &[char]) -> Option<String> {
    const WORDS: [&str; 17] = ["я", "меня", "мне", "мной", "мною", "мой", "моя", "мое", "моё", "мои", "моего", "моей", "моих",
                               "моим", "моими", "моему", "мою"];
    let joined = |character: char| is_word(character) || character == '-';
    for at in 0..text.len() {
        if at > 0 && joined(text[at - 1]) {
            continue;
        }
        for word in WORDS {
            let end = at + word.chars().count();
            if starts_folded(text, at, word) && (end == text.len() || !joined(text[end])) {
                return Some(piece(text, at, end));
            }
        }
    }
    None
}

/// His own words, quoted, each as one space: `«[^»]*»|„[^“]*“|"[^"]*"` (check.QUOTED).
fn unquoted(text: &str) -> String {
    let characters: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut position = 0;
    while position < characters.len() {
        let close = match characters[position] {
            '«' => Some('»'),
            '„' => Some('“'),
            '"' => Some('"'),
            _ => None,
        };
        if let Some(close) = close {
            if let Some(end) = (position + 1..characters.len()).find(|&at| characters[at] == close) {
                out.push(' ');
                position = end + 1;
                continue;
            }
        }
        out.push(characters[position]);
        position += 1;
    }
    out
}

/// `^\s*(\d+[a-zа-я]?)[.)]\s+(.*)$`: the number an item's text begins with, and the rest (check.ITEM_NUMBER).
fn item_number(text: &str) -> Option<(String, String)> {
    let characters: Vec<char> = text.chars().collect();
    let start = spaces_end(&characters, 0);
    let digits = run_end(&characters, start, |character| character.is_ascii_digit());
    if digits == start {
        return None;
    }
    let mut end = digits;
    if end < characters.len() && (characters[end].is_ascii_lowercase() || ('а'..='я').contains(&characters[end])) {
        end += 1;
    }
    let label = piece(&characters, start, end);
    if end >= characters.len() || !matches!(characters[end], '.' | ')') {
        return None;
    }
    let gap = end + 1;
    let rest = spaces_end(&characters, gap);
    if rest == gap {
        return None;
    }
    Some((label, piece(&characters, rest, characters.len())))
}

/// `float()` of what a pattern of digits and dots found; None where Python would have stopped with a ValueError.
fn number(text: &str) -> Option<f64> {
    text.parse::<f64>().ok()
}

// --------------------------------------------------------------------------
// A live page on the phone

/// Why a live page would break in the app on his phone, where it is half as wide as in the window; "" when
/// nothing of the kind is seen in it. A drawing shrinks to the page's width with the words drawn in it, and a
/// block of a set width does not shrink at all.
pub fn narrow_trouble(page: &str) -> String {
    let page: Vec<char> = page.chars().collect();
    for (start, end) in drawings(&page) {
        let drawing = &page[start..end];
        let has_words = (0..drawing.len()).any(|at| {
            starts_folded(drawing, at, "<text") && (at + 5 == drawing.len() || !is_word(drawing[at + 5]))
        });
        if !has_words {
            continue;
        }
        let head = &drawing[..drawing.iter().position(|&character| character == '>').unwrap_or(drawing.len())];
        let wide = match view_box_width(head).or_else(|| own_width(head)) {
            Some(found) => number(&found).unwrap_or(0.0),
            None => 0.0,
        };
        if wide > PHONE_PAGE {
            return format!("has words inside a drawing {} points wide. In the app on a phone the page is about 290 points \
                            wide, and a drawing shrinks to that with its words, past reading. Write the words as words of the page and keep a drawing to shapes; build \
                            a scheme of the kit's chain (skills/hitl/pages/scheme.html), which stands in a row in the window and \
                            one under another on the phone.", format_args!("{:.0}", wide.trunc()));
        }
    }
    let mut styles: Vec<String> = style_blocks(&page);
    styles.extend(style_attributes(&page));
    let styles: Vec<char> = without_media(&styles.join(" "));
    let joined = |character: char| is_word(character) || character == '-';
    let mut at = 0;
    while at < styles.len() {
        if at > 0 && joined(styles[at - 1]) {
            at += 1;
            continue;
        }
        let found = ["min-width", "width"].into_iter().find_map(|property| {
            if !starts_folded(&styles, at, property) {
                return None;
            }
            let colon = spaces_end(&styles, at + property.chars().count());
            if colon >= styles.len() || styles[colon] != ':' {
                return None;
            }
            let digits = spaces_end(&styles, colon + 1);
            let end = run_end(&styles, digits, |character| character.is_ascii_digit() || character == '.');
            if end == digits || !starts_folded(&styles, end, "px") {
                return None;
            }
            Some((piece(&styles, at, at + property.chars().count()), piece(&styles, digits, end), end + 2))
        });
        match found {
            Some((property, size, end)) => {
                if number(&size).is_some_and(|value| value > PHONE_PAGE) {
                    return format!("sets «{property}: {size}px». In the app on a phone the page is about 290 points wide, \
                                    and a block of that width runs off it. Let it be as wide as the page: a share of it, flex, \
                                    or max-width.");
                }
                at = end;
            }
            None => at += 1,
        }
    }
    String::new()
}

/// `re.findall(r"<svg\b.*?</svg>", page, re.DOTALL | re.IGNORECASE)`, as where each stands.
fn drawings(page: &[char]) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    let mut at = 0;
    while at < page.len() {
        if starts_folded(page, at, "<svg") && (at + 4 == page.len() || !is_word(page[at + 4])) {
            match find_folded(page, at + 4, "</svg>") {
                Some(close) => {
                    found.push((at, close + 6));
                    at = close + 6;
                    continue;
                }
                None => break,
            }
        }
        at += 1;
    }
    found
}

/// `viewBox\s*=\s*["']\s*[-\d.]+[\s,]+[-\d.]+[\s,]+([\d.]+)`, ignoring case: the box's width.
fn view_box_width(head: &[char]) -> Option<String> {
    let number_part = |character: char| character == '-' || character == '.' || character.is_ascii_digit();
    let gap = |character: char| character == ',' || py_space(character);
    (0..head.len()).find_map(|at| {
        if !starts_folded(head, at, "viewbox") {
            return None;
        }
        let equals = spaces_end(head, at + 7);
        if equals >= head.len() || head[equals] != '=' {
            return None;
        }
        let quote = spaces_end(head, equals + 1);
        if quote >= head.len() || !matches!(head[quote], '"' | '\'') {
            return None;
        }
        let mut position = spaces_end(head, quote + 1);
        for member in [&number_part as &dyn Fn(char) -> bool, &gap, &number_part, &gap] {
            let end = run_end(head, position, member);
            if end == position {
                return None;
            }
            position = end;
        }
        let end = run_end(head, position, |character| character == '.' || character.is_ascii_digit());
        (end > position).then(|| piece(head, position, end))
    })
}

/// `(?<![-\w])width\s*=\s*["']([\d.]+)`, ignoring case: the drawing's own width.
fn own_width(head: &[char]) -> Option<String> {
    (0..head.len()).find_map(|at| {
        if at > 0 && (is_word(head[at - 1]) || head[at - 1] == '-') || !starts_folded(head, at, "width") {
            return None;
        }
        let equals = spaces_end(head, at + 5);
        if equals >= head.len() || head[equals] != '=' {
            return None;
        }
        let quote = spaces_end(head, equals + 1);
        if quote >= head.len() || !matches!(head[quote], '"' | '\'') {
            return None;
        }
        let end = run_end(head, quote + 1, |character| character == '.' || character.is_ascii_digit());
        (end > quote + 1).then(|| piece(head, quote + 1, end))
    })
}

/// `re.findall(r"<style\b[^>]*>(.*?)</style>", page, re.DOTALL | re.IGNORECASE)`.
fn style_blocks(page: &[char]) -> Vec<String> {
    let mut found = Vec::new();
    let mut at = 0;
    while at < page.len() {
        if starts_folded(page, at, "<style") && (at + 6 == page.len() || !is_word(page[at + 6])) {
            let Some(opened) = (at + 6..page.len()).find(|&position| page[position] == '>') else { break };
            let Some(close) = find_folded(page, opened + 1, "</style>") else { break };
            found.push(piece(page, opened + 1, close));
            at = close + 8;
            continue;
        }
        at += 1;
    }
    found
}

/// `re.findall(r"\bstyle\s*=\s*\"([^\"]*)\"", page, re.IGNORECASE)`.
fn style_attributes(page: &[char]) -> Vec<String> {
    let mut found = Vec::new();
    let mut at = 0;
    while at < page.len() {
        if (at == 0 || !is_word(page[at - 1])) && starts_folded(page, at, "style") {
            let equals = spaces_end(page, at + 5);
            if equals < page.len() && page[equals] == '=' {
                let quote = spaces_end(page, equals + 1);
                if quote < page.len() && page[quote] == '"' {
                    if let Some(close) = (quote + 1..page.len()).find(|&position| page[position] == '"') {
                        found.push(piece(page, quote + 1, close));
                        at = close + 1;
                        continue;
                    }
                }
            }
        }
        at += 1;
    }
    found
}

/// `re.sub(r"@media[^{]*", "", styles)`: what a rule is for, not what it sets.
fn without_media(styles: &str) -> Vec<char> {
    let characters: Vec<char> = styles.chars().collect();
    let mut out = Vec::new();
    let mut at = 0;
    while at < characters.len() {
        if starts(&characters, at, "@media") {
            at = run_end(&characters, at + 6, |character| character != '{');
            continue;
        }
        out.push(characters[at]);
        at += 1;
    }
    out
}

// --------------------------------------------------------------------------
// Optional task notes

/// Task notes keep their number when renamed or moved. r/NN is a legacy chat name for
/// team/NN; both open the same mirrored note.
pub fn cards() -> PathBuf {
    match common::var("HITL_CARDS").filter(|value| !value.is_empty())
        .or_else(|| common::var("CONVEYOR_CARDS").filter(|value| !value.is_empty())) {
        Some(root) => PathBuf::from(root),
        None => common::home().join("cards"),
    }
}

/// Read only the mirror's number scalar in YAML frontmatter, never a number in the body. The plugin writes this field
/// as plain text; quoted scalars work too. A missing closing delimiter or a repeated number field is not a reliable
/// task identity.
pub fn note_number(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let bytes = bytes.strip_prefix("\u{feff}".as_bytes()).unwrap_or(&bytes);
    let mut lines = Vec::new();
    let mut start = 0;
    let mut position = 0;
    while position < bytes.len() {
        match bytes[position] {
            b'\n' => {
                lines.push(&bytes[start..position]);
                start = position + 1;
            }
            b'\r' => {
                lines.push(&bytes[start..position]);
                if bytes.get(position + 1) == Some(&b'\n') {
                    position += 1;
                }
                start = position + 1;
            }
            _ => {}
        }
        position += 1;
    }
    if start < bytes.len() {
        lines.push(&bytes[start..]);
    }
    let mut lines = lines.into_iter().map(std::str::from_utf8);
    let first = lines.next().unwrap_or(Ok("")).ok()?;
    if strip(first) != "---" {
        return None;
    }
    let mut found: Vec<String> = Vec::new();
    for line in lines {
        let line = line.ok()?;
        if strip(line) == "---" {
            return if found.len() == 1 { found.pop() } else { None };
        }
        if line.starts_with("number:") {
            found.push(number_field(line)?);
        }
    }
    None
}

/// `number:[ \t]*(?:"(team/[1-9][0-9]*)"|'(team/[1-9][0-9]*)'|(team/[1-9][0-9]*))[ \t]*(?:#.*)?`, the whole line.
fn number_field(line: &str) -> Option<String> {
    let blank = |character: char| character == ' ' || character == '\t';
    let rest = line.strip_prefix("number:")?.trim_start_matches(blank);
    let (quote, rest) = match rest.chars().next() {
        Some(quote @ ('"' | '\'')) => (Some(quote), &rest[1..]),
        _ => (None, rest),
    };
    let digits = rest.strip_prefix("team/")?;
    let count = digits.chars().take_while(char::is_ascii_digit).count();
    if count == 0 || digits.starts_with('0') {
        return None;
    }
    let value = format!("team/{}", &digits[..count]);
    let mut rest = &digits[count..];
    if let Some(quote) = quote {
        rest = rest.strip_prefix(quote)?;
    }
    let rest = rest.trim_start_matches(blank);
    (rest.is_empty() || rest.starts_with('#')).then_some(value)
}

/// Find the note by its task number, not the filename, title, topic or old pointer. No arbitrary choice when a
/// mirror accidentally contains two notes with the same number. None outside hitl.
pub fn card_of(tag: &str) -> Option<Value> {
    if !common::is_host() {
        return None;
    }
    card_in(tag, &cards())
}

fn card_in(tag: &str, root: &Path) -> Option<Value> {
    let lowered = strip(tag).to_lowercase();
    let (prefix, digits) = lowered.split_once('/')?;
    if !matches!(prefix, "r" | "team") || digits.is_empty() || !digits.chars().all(|character| character.is_ascii_digit()) {
        return None;
    }
    let number = digits.trim_start_matches('0');
    let number = if number.is_empty() { "0" } else { number };
    let wanted = format!("team/{number}");
    let mut found = Vec::new();
    notes(root, &mut found);
    let found: Vec<PathBuf> = found.into_iter().filter(|path| note_number(path).as_deref() == Some(wanted.as_str())).collect();
    if found.len() != 1 {
        return None;
    }
    Some(json!({"title": format!("карточка {number}"), "path": found[0].display().to_string()}))
}

/// `root.rglob("*.md")`: every entry so named, in folders that are not links.
fn notes(folder: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(folder) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_name().to_string_lossy().ends_with(".md") {
            found.push(path.clone());
        }
        let link = entry.file_type().map(|kind| kind.is_symlink()).unwrap_or(false);
        if !link && path.is_dir() {
            notes(&path, found);
        }
    }
}

// --------------------------------------------------------------------------
// Files named on the command line

/// `Path(text).expanduser()`, as pathlib reads it: a trailing slash and `.` parts dropped.
fn py_path(text: &str) -> PathBuf {
    let expanded = expand_user(text);
    Path::new(&expanded).components().collect()
}

fn expand_user(text: &str) -> String {
    let Some(rest) = text.strip_prefix('~') else { return text.to_string() };
    let (user, tail) = match rest.find('/') {
        Some(slash) => (&rest[..slash], &rest[slash..]),
        None => (rest, ""),
    };
    let home = if user.is_empty() {
        common::var("HOME").or_else(|| home_of(None))
    } else {
        home_of(Some(user))
    };
    match home {
        Some(home) => {
            let joined = format!("{}{tail}", home.trim_end_matches('/'));
            if joined.is_empty() { "/".into() } else { joined }
        }
        None => text.to_string(),
    }
}

/// The home folder of a user, from the system's list of users; the caller's own for None.
fn home_of(user: Option<&str>) -> Option<String> {
    let entry = match user {
        Some(name) => {
            let name = std::ffi::CString::new(name).ok()?;
            unsafe { libc::getpwnam(name.as_ptr()) }
        }
        None => unsafe { libc::getpwuid(libc::getuid()) },
    };
    if entry.is_null() {
        return None;
    }
    let directory = unsafe { (*entry).pw_dir };
    if directory.is_null() {
        return None;
    }
    Some(unsafe { std::ffi::CStr::from_ptr(directory) }.to_string_lossy().to_string())
}

/// `Path.name`.
fn py_name(path: &Path) -> String {
    match path.components().next_back() {
        Some(Component::Normal(name)) => name.to_string_lossy().to_string(),
        Some(Component::ParentDir) => "..".into(),
        _ => String::new(),
    }
}

/// `Path.suffix.lower()`.
fn py_suffix(path: &Path) -> String {
    let name = py_name(path);
    match name.rfind('.') {
        Some(dot) if dot > 0 && dot < name.len() - 1 => name[dot..].to_lowercase(),
        _ => String::new(),
    }
}

/// `str(Path.resolve())` of a file that is there.
fn resolved(path: &Path) -> String {
    match path.canonicalize() {
        Ok(found) => found.display().to_string(),
        Err(_) => std::env::current_dir().map(|here| here.join(path)).unwrap_or_else(|_| path.to_path_buf()).display().to_string(),
    }
}

/// Whether a file is there. What is given as a link may be an address too long to be a file's name: asked as a file
/// it must not stop the command, as a link to a new chat of Codex with its request in it did on 2026-10-10.
pub fn is_file(path: &Path) -> bool {
    std::fs::metadata(path).map(|meta| meta.is_file()).unwrap_or(false)
}

pub fn is_there(path: &Path) -> bool {
    std::fs::metadata(path).is_ok()
}

/// Why a link written into a text would not open; "" when every one would, or there is none.
pub fn links_trouble(text: &str) -> String {
    for (words, target) in common::text_links(text) {
        if !common::is_address(&target) {
            return format!("«[{words}]({target})» reads as a link, and «{target}» is nothing to open. A link in a text is \
                            [слова](адрес), where the address is a page (https://…), another app's own address (obsidian://…) or \
                            the full path of a file on this Mac (/Users/… or ~/…).");
        }
        if (target.starts_with('/') || target.starts_with("~/")) && !is_there(&py_path(&target)) {
            return format!("the link «{words}» leads to {target}, and there is no such file.");
        }
    }
    String::new()
}

// --------------------------------------------------------------------------
// The options

/// What a chat is told when its call is refused: `{tool} check: {message}` on stderr, and 64.
pub fn refuse(message: &str) -> i32 {
    common::say(&format!("{} check: {message}", common::tool()));
    common::EXIT_REFUSED
}

fn read_length(text: &str) -> usize {
    common::length(&common::read_text(text))
}

fn linked(place: &str, text: &str) -> Result<(), String> {
    let trouble = links_trouble(text);
    if trouble.is_empty() { Ok(()) } else { Err(format!("{place}: {trouble}")) }
}

fn unlinked(place: &str, text: &str, why: &str) -> Result<(), String> {
    if common::text_links(text).is_empty() {
        Ok(())
    } else {
        Err(format!("{place} has a link in it. {why} A link goes into the text of an item, its --details or a --fact."))
    }
}

fn value(arguments: &[String], position: &mut usize, flag: &str) -> Result<String, String> {
    *position += 1;
    arguments.get(*position).cloned().ok_or_else(|| format!("{flag} needs a value."))
}

fn last(items: &[Value], flag: &str) -> Result<usize, String> {
    if items.is_empty() { Err(format!("{flag} goes after the --check it belongs to.")) } else { Ok(items.len() - 1) }
}

/// An item's number, or its place: the place of the first item equal to it, as Python's `list.index` finds it.
fn name(items: &[Value], index: usize) -> String {
    let label = common::text_of(&items[index], "label");
    if !label.is_empty() {
        return label.to_string();
    }
    let place = items.iter().position(|item| *item == items[index]).unwrap_or(index);
    (place + 1).to_string()
}

fn field<'a>(items: &'a mut [Value], index: usize) -> &'a mut Map<String, Value> {
    items[index].as_object_mut().expect("an item is an object")
}

/// `item.setdefault(key, [])`.
fn list<'a>(items: &'a mut [Value], index: usize, key: &str) -> &'a mut Vec<Value> {
    field(items, index).entry(key.to_string()).or_insert_with(|| Value::Array(Vec::new())).as_array_mut().expect("a list")
}

fn holds(items: &[Value], index: usize, key: &str, text: &str) -> bool {
    common::list_of(&items[index], key).iter().any(|value| value.as_str() == Some(text))
}

/// A number as Python's int() gives it back as text: no leading zeros.
fn plain_number(digits: &str) -> String {
    let trimmed = digits.trim_start_matches('0');
    if trimmed.is_empty() { "0".into() } else { trimmed.into() }
}

/// The larger of two numbers written in digits, of any length.
fn larger(left: &str, right: &str) -> String {
    let (left, right) = (plain_number(left), plain_number(right));
    if (left.len(), left.as_str()) >= (right.len(), right.as_str()) { left } else { right }
}

/// A number written in digits, and one.
fn next_number(digits: &str) -> String {
    let mut out: Vec<u8> = plain_number(digits).into_bytes();
    let mut position = out.len();
    loop {
        if position == 0 {
            out.insert(0, b'1');
            break;
        }
        position -= 1;
        if out[position] == b'9' {
            out[position] = b'0';
        } else {
            out[position] += 1;
            break;
        }
    }
    String::from_utf8(out).expect("digits")
}

/// The options read and held to the limits; Err is the refusal a chat is told, without the «hitl check: » head.
pub fn parse_submission(arguments: &[String]) -> Result<Value, String> {
    parse(arguments, common::is_host())
}

fn parse(arguments: &[String], hitl: bool) -> Result<Value, String> {
    let mut title = String::new();
    let mut about = String::new();
    let mut kind = "checklist".to_string();
    let mut items: Vec<Value> = Vec::new();
    let mut tag: Option<String> = None;
    let mut wait = true;
    let mut after: Option<String> = None;
    let mut position = 0;
    while position < arguments.len() {
        let flag = arguments[position].as_str();
        match flag {
            "--title" => {
                let text = squeeze(&value(arguments, &mut position, flag)?);
                unlinked("--title", &text, "The title names the check, in the queue and in a notification too.")?;
                title = text;
                let count = common::length(&title);
                if count > MAX_TITLE {
                    return Err(format!("--title is {count} characters long, the limit is {MAX_TITLE}: it names the check in a few words."));
                }
            }
            "--about" => {
                let text = squeeze(&value(arguments, &mut position, flag)?);
                linked("--about", &text)?;
                about = text;
                let count = read_length(&about);
                if count > MAX_ABOUT {
                    return Err(format!("--about is {count} characters long, the limit is {MAX_ABOUT}: one plain sentence about the task."));
                }
            }
            "--kind" => {
                kind = value(arguments, &mut position, flag)?;
                if kind_title(&kind).is_none() {
                    return Err(format!("--kind is one of: {}.", KINDS.map(|(name, _)| name).join(", ")));
                }
            }
            "--check" => {
                let text = squeeze(&value(arguments, &mut position, flag)?);
                let (label, text) = match item_number(&text) {
                    Some((label, rest)) => (label, strip(&rest).to_string()),
                    None => (String::new(), text),
                };
                if text.is_empty() {
                    return Err("--check needs a text.".into());
                }
                let number = if label.is_empty() { (items.len() + 1).to_string() } else { label.clone() };
                linked(&format!("item {number}"), &text)?;
                let count = read_length(&text);
                if count > MAX_ITEM {
                    return Err(format!("item {number} is {count} characters long, the limit is {MAX_ITEM}. {SPLIT}"));
                }
                items.push(json!({"label": label, "text": text}));
            }
            "--details" => {
                let index = last(&items, flag)?;
                let text = strip(&value(arguments, &mut position, flag)?).to_string();
                if items[index].get("details").is_some() {
                    return Err(format!("item {} already has its --details.", name(&items, index)));
                }
                if let Some(file) = text.strip_prefix('@') {
                    if is_file(&py_path(file)) {
                        return Err("--details does not read a file: a text that long is not read. Give the file as --link and \
                                    say in a line what to look at.".into());
                    }
                }
                let lines: Vec<String> = splitlines(&text).into_iter().map(squeeze).filter(|line| !line.is_empty()).collect();
                let text = lines.join("\n");
                linked(&format!("the details of item {}", name(&items, index)), &text)?;
                let count = read_length(&text);
                if count > MAX_DETAILS || lines.len() > MAX_DETAIL_LINES {
                    return Err(format!("the details of item {} are {count} characters in {} lines, the limit is {MAX_DETAILS} \
                                        characters and {MAX_DETAIL_LINES} lines. {SPLIT}", name(&items, index), lines.len()));
                }
                if !text.is_empty() {
                    field(&mut items, index).insert("details".into(), Value::String(text));
                }
            }
            "--fact" => {
                let index = last(&items, flag)?;
                let text = squeeze(&value(arguments, &mut position, flag)?);
                if text.is_empty() {
                    return Err("--fact needs a text.".into());
                }
                linked(&format!("a fact of item {}", name(&items, index)), &text)?;
                let count = read_length(&text);
                if count > MAX_FACT {
                    return Err(format!("a fact of item {} is {count} characters long, the limit is {MAX_FACT}: a fact is one \
                                        line. Cut it into two facts, or into several items.", name(&items, index)));
                }
                list(&mut items, index, "facts").push(Value::String(text));
                if common::list_of(&items[index], "facts").len() > MAX_FACTS {
                    return Err(format!("item {} has more than {MAX_FACTS} facts: an item is read in seconds. Cut the explanation \
                                        into several items, each with its own facts.", name(&items, index)));
                }
            }
            "--link" | "--image" => {
                let index = last(&items, flag)?;
                let target = value(arguments, &mut position, flag)?;
                let file = py_path(&target);
                let image = flag == "--image";
                if image && !is_file(&file) {
                    return Err(format!("--image: there is no file {target}."));
                }
                let picture = PICTURES.contains(&py_suffix(&file).as_str());
                if image && !picture {
                    return Err(format!("--image takes a picture ({}); give {} as --link.", PICTURES.join(", "), py_name(&file)));
                }
                if is_file(&file) && picture {
                    // A picture is shown in the window itself, whichever way it was given.
                    list(&mut items, index, "images").push(Value::String(resolved(&file)));
                    if common::list_of(&items[index], "images").len() > MAX_IMAGES {
                        return Err(format!("item {} has more than {MAX_IMAGES} pictures: put the rest on another item.", name(&items, index)));
                    }
                } else {
                    list(&mut items, index, "links").push(Value::String(target));
                    if common::list_of(&items[index], "links").len() > MAX_LINKS {
                        return Err(format!("item {} has more than {MAX_LINKS} links: what else is there to open belongs to another \
                                            item.", name(&items, index)));
                    }
                }
            }
            "--copy" => {
                let index = last(&items, flag)?;
                let text = strip(&value(arguments, &mut position, flag)?).to_string();
                if text.is_empty() {
                    return Err("--copy needs a text.".into());
                }
                if text.contains('\n') || text.contains('\r') {
                    return Err(format!("a line to copy of item {} has several lines: one line is copied with one click. Give each \
                                        line as a --copy of its own, or put a script into a file and give the command that runs it.",
                                       name(&items, index)));
                }
                let count = common::length(&text);
                if count > MAX_COPY {
                    return Err(format!("a line to copy of item {} is {count} characters long, the limit is {MAX_COPY}: put what is \
                                        longer into a file and give the command that runs it, or the file as --link.", name(&items, index)));
                }
                list(&mut items, index, "copies");
                if holds(&items, index, "copies", &text) {
                    return Err(format!("item {} gives «{text}» to copy twice.", name(&items, index)));
                }
                list(&mut items, index, "copies").push(Value::String(text));
                if common::list_of(&items[index], "copies").len() > MAX_COPIES {
                    return Err(format!("item {} has more than {MAX_COPIES} lines to copy: one item is one step. Put the next \
                                        command into the next item.", name(&items, index)));
                }
            }
            "--page" => {
                let index = last(&items, flag)?;
                let target = value(arguments, &mut position, flag)?;
                let file = py_path(&target);
                if items[index].get("page").is_some() {
                    return Err(format!("item {} already has its page: one page an item.", name(&items, index)));
                }
                if !is_file(&file) {
                    return Err(format!("--page: there is no file {target}."));
                }
                if !PAGES.contains(&py_suffix(&file).as_str()) {
                    return Err(format!("--page takes an HTML file; give {} as --link or --image.", py_name(&file)));
                }
                let size = std::fs::metadata(&file).map(|meta| meta.len()).unwrap_or(0);
                if size > MAX_PAGE_BYTES {
                    return Err(format!("the page of item {} is {} KB, the limit is {} KB: a page is a few switches and what they \
                                        change, whole in one file.", name(&items, index), size / 1000, MAX_PAGE_BYTES / 1000));
                }
                // He reads the same page in the app on his phone: one that would break there does not reach him.
                let page = std::fs::read(&file).unwrap_or_default();
                let trouble = narrow_trouble(&String::from_utf8_lossy(&page));
                if !trouble.is_empty() {
                    return Err(format!("the page of item {} {trouble}", name(&items, index)));
                }
                field(&mut items, index).insert("page".into(), Value::String(resolved(&file)));
            }
            "--option" => {
                let index = last(&items, flag)?;
                let text = squeeze(&value(arguments, &mut position, flag)?);
                if text.is_empty() {
                    return Err("--option needs a text.".into());
                }
                unlinked(&format!("an option of item {}", name(&items, index)), &text,
                         "An option is a button that answers: a click on it is the answer.")?;
                let count = common::length(&text);
                if count > MAX_OPTION {
                    return Err(format!("an option of item {} is {count} characters long, the limit is {MAX_OPTION}: an option is \
                                        a few words on a button. The reasons go into --details.", name(&items, index)));
                }
                list(&mut items, index, "options");
                if holds(&items, index, "options", &text) {
                    return Err(format!("item {} offers «{text}» twice.", name(&items, index)));
                }
                list(&mut items, index, "options").push(Value::String(text));
            }
            "--many" => {
                let index = last(&items, flag)?;
                field(&mut items, index).insert("many".into(), Value::Bool(true));
            }
            "--as" => {
                let index = last(&items, flag)?;
                let own = value(arguments, &mut position, flag)?;
                if own == "result" {
                    return Err(format!("item {}: a result is a check of its own (--kind result): an «ок» to it is the word for \
                                        a merge, and it does not stand among questions and actions.", name(&items, index)));
                }
                if kind_title(&own).is_none() {
                    let others: Vec<&str> = KINDS.iter().map(|(name, _)| *name).filter(|name| *name != "result").collect();
                    return Err(format!("--as is one of: {}.", others.join(", ")));
                }
                field(&mut items, index).insert("kind".into(), Value::String(own));
            }
            "--after" => after = Some(strip(&value(arguments, &mut position, flag)?).to_string()),
            "--tag" => tag = Some(strip(&value(arguments, &mut position, flag)?).to_string()),
            "--no-wait" => wait = false,
            _ => return Err(format!("unknown option {flag}. Run {} check --help.", common::tool())),
        }
        position += 1;
    }

    if items.is_empty() {
        return Err("nothing to check: give at least one --check \"что сделать → что должно быть\".".into());
    }
    if title.is_empty() {
        return Err("--title says in a few words what the check is about.".into());
    }
    if about.is_empty() {
        return Err(ABOUT.into());
    }
    for index in 0..items.len() {
        let options = items[index].get("options").map(|_| common::list_of(&items[index], "options").len());
        if common::truthy(items[index].get("many")) && options.is_none() {
            return Err(format!("item {} has --many and no options: --many says that several of the item's own --option may \
                                hold at once.", name(&items, index)));
        }
        if let Some(count) = options {
            if !(2..=MAX_OPTIONS).contains(&count) {
                return Err(format!("item {} offers {count} options: a question has 2 to {MAX_OPTIONS} answers to choose from. \
                                    A yes or no is --kind question without options.", name(&items, index)));
            }
        }
    }
    for index in 0..items.len() {
        let own = common::text_of(&items[index], "kind").to_string();
        if !own.is_empty() && kind == "result" {
            return Err(format!("item {} has --as in a result: an «ок» to a result is the word for a merge, so a result holds \
                                nothing else. Put the question or the action into a check of its own.", name(&items, index)));
        }
        if own == kind {
            field(&mut items, index).shift_remove("kind");
        } else if !own.is_empty() {
            // The word shown at the top and the names of its answers go with the item.
            let item = field(&mut items, index);
            item.insert("kindTitle".into(), json!(kind_title(&own).unwrap_or("")));
            item.insert("answers".into(), json!(answers(&own)));
        }
    }
    // What he reads, piece by piece, with where it stands and whether it explains: the rules his «не понял» has
    // made are held here.
    let mut texts: Vec<(String, String, bool)> = vec![("--title".into(), title.clone(), false),
                                                      ("--about".into(), about.clone(), kind == "explain")];
    for index in 0..items.len() {
        let item = &items[index];
        let place = format!("item {}", name(&items, index));
        let own = common::text_of(item, "kind");
        let explains = if own.is_empty() { kind == "explain" } else { own == "explain" };
        texts.push((place.clone(), common::text_of(item, "text").to_string(), explains));
        texts.push((format!("the details of {place}"), common::text_of(item, "details").to_string(), explains));
        for fact in common::list_of(item, "facts") {
            texts.push((format!("a fact of {place}"), fact.as_str().unwrap_or("").to_string(), explains));
        }
        for option in common::list_of(item, "options") {
            texts.push((format!("an option of {place}"), option.as_str().unwrap_or("").to_string(), false));
        }
    }
    for (place, text, explains) in &texts {
        let own: Vec<char> = unquoted(&common::read_text(text)).chars().collect();
        if hitl {
            // His words, not his team's.
            for (pattern, instead, why) in HIS_UNKNOWN_WORDS {
                if let Some(found) = pattern(&own) {
                    return Err(format!("{place}: «{found}» is a word he does not understand ({why}): say «{instead}». A word he \
                                        did not say himself is explained in the same item or replaced by a plain one. His own \
                                        words in «…» are left alone."));
                }
            }
        }
        if *explains {
            if let Some(found) = first_person(&own) {
                return Err(format!("{place}: «{found}» in an explanation. The person holds many chats, and several agents may stand \
                                    behind that word: name who exactly does what, by what it is for the person: the chat by its \
                                    name, the agent, the command, the app. The person's own words in «…» are left alone."));
            }
        }
    }
    let mut used: Vec<String> = Vec::new();
    let mut counter = "0".to_string();
    for index in 0..items.len() {
        let label = common::text_of(&items[index], "label").to_string();
        let label = if label.is_empty() {
            // Items without a number continue the count.
            counter = next_number(&counter);
            while used.contains(&counter) {
                counter = next_number(&counter);
            }
            field(&mut items, index).insert("label".into(), Value::String(counter.clone()));
            counter.clone()
        } else {
            if label.chars().all(|character| character.is_ascii_digit()) {
                counter = larger(&counter, &label);
            }
            label
        };
        if used.contains(&label) {
            return Err(format!("two items share the number {label}."));
        }
        used.push(label);
    }
    if let Some(earlier_id) = after.as_deref().filter(|after| !after.is_empty()) {
        let Some(earlier) = verdict::ended(earlier_id).filter(|earlier| common::truthy(Some(earlier))) else {
            return Err(format!("--after {earlier_id}: there is no ended check with that number. It is the number the verdict of \
                                the earlier check began with; `{} queue` lists the recent ones.", common::tool()));
        };
        let said = verdict::said_before(&earlier);
        let said = said.as_object().cloned().unwrap_or_default();
        for index in 0..items.len() {
            let label = common::text_of(&items[index], "label").to_string();
            if let Some(before) = said.get(&label) {
                field(&mut items, index).insert("before".into(), before.clone());
            }
        }
        if !items.iter().any(|item| item.get("before").is_some()) {
            let numbers: Vec<&str> = said.keys().map(String::as_str).collect();
            let numbers = if numbers.is_empty() { "there are none".to_string() } else { numbers.join(", ") };
            return Err(format!("--after {earlier_id}: none of these items has the number of an item the person wrote to or did not say \
                                yes to there ({numbers}). An item asked again keeps its number: that is how the last answer \
                                finds it."));
        }
    }
    Ok(json!({"title": title, "about": about, "kind": kind, "items": items, "tag": tag, "wait": wait, "after": after}))
}

// --------------------------------------------------------------------------
// The ticket

/// A number for the check that no other check has, in the queue or among the ended ones.
pub fn new_id(tag: &str) -> String {
    common::new_id(tag, |ticket_id| {
        window::queue_folder().join(format!("{ticket_id}.json")).exists() || window::done_folder().join(format!("{ticket_id}.json")).exists()
    })
}

/// How the window names the chat a check comes from (checknames.presentation): the chat's own app asked by the
/// program itself, `check --presentation`, the way the window asks it. The recorded title stays where that fails.
pub fn presentation(ticket: &Value) -> Value {
    let fallback = json!({"chat": common::text_of(ticket, "chat"), "reference": "", "status": "unidentified"});
    let app = common::text_of(ticket, "agent");
    let ids = common::list_of(ticket, "chatIds");
    if !["Claude", "Codex", "OpenCode"].contains(&app) || (!common::truthy(ticket.get("chatRef")) && ids.is_empty()) || cfg!(test) {
        return fallback;
    }
    // Where Python would stop on what it cannot read, the check keeps the name it was asked by.
    crate::names::presentation(ticket, &common::env()).ok().filter(Value::is_object).unwrap_or(fallback)
}

/// The ticket written into the queue (check.submit).
pub fn submit(parsed: Value) -> Value {
    let environment = common::env();
    let title = common::chat_title(&environment);
    let prefix = common::chat_prefix(&title, 16);
    let tag = match parsed.get("tag") {
        Some(Value::String(named)) => named.clone(),
        _ => prefix,
    };
    let top = common::git(&["rev-parse", "--show-toplevel"]);
    let (branch, commit) = if top.is_empty() {
        (String::new(), String::new())
    } else {
        (common::git(&["rev-parse", "--abbrev-ref", "HEAD"]), common::git(&["rev-parse", "HEAD"]))
    };
    let kind = common::text_of(&parsed, "kind").to_string();
    let mut ticket = Map::new();
    ticket.insert("id".into(), json!(new_id(&tag)));
    ticket.insert("screen".into(), json!("mac"));
    ticket.insert("tag".into(), json!(tag));
    ticket.insert("chat".into(), json!(title));
    // What opens the chat, when it has an address: the window offers it at every item.
    ticket.insert("chatLink".into(), json!(common::chat_link(&environment)));
    // Which of the projects it is about, by the name the person knows: shown first in the window.
    ticket.insert("projectName".into(), json!(common::project_name(None)));
    ticket.insert("title".into(), parsed.get("title").cloned().unwrap_or(json!("")));
    ticket.insert("about".into(), parsed.get("about").cloned().unwrap_or(json!("")));
    ticket.insert("card".into(), card_of(&tag).unwrap_or(Value::Null));
    // Where the check comes from, for when the chat's name does not say or there is none.
    ticket.insert("agent".into(), json!(common::agent(&environment)));
    // The ids the chat's app gave the command: the app counts by them what waits from each chat.
    ticket.insert("chatIds".into(), json!(tell::chat_ids(&environment)));
    if let Some(base) = environment.get("T3CODE_HOME").filter(|path| !path.is_empty()) {
        ticket.insert("t3Base".into(), json!(base));
    }
    ticket.insert("cwd".into(), json!(std::env::current_dir().map(|here| here.display().to_string()).unwrap_or_default()));
    ticket.insert("kind".into(), json!(kind));
    ticket.insert("kindTitle".into(), json!(kind_title(&kind).unwrap_or("")));
    ticket.insert("answers".into(), json!(answers(&kind)));
    ticket.insert("project".into(), json!(top));
    ticket.insert("branch".into(), json!(branch));
    ticket.insert("commit".into(), json!(commit));
    ticket.insert("items".into(), parsed.get("items").cloned().unwrap_or(json!([])));
    ticket.insert("createdAt".into(), json!(common::now()));
    let mut ticket = Value::Object(ticket);
    let label = presentation(&ticket);
    if common::text_of(&label, "status") == "active" {
        let map = ticket.as_object_mut().expect("a ticket is an object");
        map.insert("chat".into(), label.get("chat").cloned().unwrap_or(Value::Null));
        map.insert("chatRef".into(), label.get("reference").cloned().unwrap_or(Value::Null));
    }
    let ticket_id = common::text_of(&ticket, "id").to_string();
    if let Err(error) = common::write_json(&window::queue_folder().join(format!("{ticket_id}.json")), &ticket) {
        common::say(&format!("{} check: {error}", common::tool()));
        common::exit(1);
    }
    common::emit("hitl.queued", json!({"request": ticket_id, "source": "hitl",
                                       "context": {"kind": kind, "items": common::list_of(&ticket, "items").len()}}));
    ticket
}

fn command_submit(arguments: &[String]) -> i32 {
    let parsed = match parse_submission(arguments) {
        Ok(parsed) => parsed,
        Err(message) => return refuse(&message),
    };
    let wait = parsed.get("wait").and_then(Value::as_bool).unwrap_or(true);
    let ticket = submit(parsed);
    let ticket_id = common::text_of(&ticket, "id").to_string();
    queue::clean_done();
    let count = common::list_of(&ticket, "items").len() as i64;
    let tag = Some(common::text_of(&ticket, "tag")).filter(|tag| !tag.is_empty()).unwrap_or("чат");
    common::say(&format!("==> Проверка {tag} «{}» ({count} {}, {}) принята, номер {ticket_id}.", common::text_of(&ticket, "title"),
                         common::plural(count, "пункт", "пункта", "пунктов"),
                         kind_title(common::text_of(&ticket, "kind")).unwrap_or("")));
    // A chat nothing wakes gets its verdict as a message: «Ответил в окне, а чат Codex молчит». Its messenger starts
    // before the window, which may take a minute to build.
    let environment = common::env();
    let mut who = tell::addressee(&environment);
    if let Some(addressee) = &who {
        if !tell::start(&ticket, addressee) {
            who = None;
        }
    }
    if who.is_none() {
        let left = tell::left_waiting(&ticket_id);
        if !left.is_empty() {
            common::say(&left);
        }
        // A chat that waits itself can lose the wake: the app stops the command that waits. Who asked is kept until
        // that chat has printed the verdict, and it is reminded of an answer it has not read. On hitl this also
        // tells which chat of Claude Code asked, for its protection alone (note.remember).
        tell::remember(&ticket);
    }
    // No notification: the window cannot be put away, so a check in it is already in front of the person.
    window::ensure_panel();
    if let Some(addressee) = who {
        common::say(&tell::note(&addressee, &ticket_id));
        println!("{ticket_id}");
        return queue::EXIT_QUEUED;
    }
    if !wait {
        common::say(&format!("==> Проверка в очереди. Итог: {} wait {ticket_id}", common::tool()));
        println!("{ticket_id}");
        return queue::EXIT_QUEUED;
    }
    // A shell-backgrounded check may only write to a log. That is not a Bash task notification or a read by Claude.
    // Keep its messenger until native output is observed, or its own explicit `wait` takes it.
    queue::wait_for(&ticket_id, common::agent(&environment) != "Claude")
}

/// `hitl check ...` (check.main and check.command_submit).
pub fn main(arguments: &[String]) -> i32 {
    if arguments == ["--presentation"] {
        return crate::names::main();
    }
    if arguments.is_empty() || matches!(arguments[0].as_str(), "-h" | "--help" | "help") {
        println!("{}", usage());
        return 0;
    }
    if !cfg!(target_os = "macos") && common::var("CONVEYOR_WINDOW").as_deref() != Some("") {
        return refuse("the check window is for macOS.");
    }
    queue::prepare();
    command_submit(arguments)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(text: &str) -> Vec<char> {
        text.chars().collect()
    }

    fn arguments(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| word.to_string()).collect()
    }

    fn refused(words: &[&str]) -> String {
        parse(&arguments(words), true).expect_err("refused")
    }

    const BASE: [&str; 4] = ["--title", "Вход", "--about", "Плагин входит без пароля."];

    fn with_base(words: &[&str]) -> Vec<String> {
        BASE.iter().chain(words).map(|word| word.to_string()).collect()
    }

    #[test]
    fn his_unknown_words_are_found_as_python_finds_them() {
        let landing = |text: &str| landing_word(&chars(text));
        let folder = |text: &str| live_folder(&chars(text));
        assert_eq!(landing("Посадка ветки").as_deref(), Some("Посадка"));
        assert_eq!(landing("посадила все").as_deref(), Some("посадила"));
        assert_eq!(landing("посадили").as_deref(), Some("посадили"));
        assert_eq!(landing("посади").as_deref(), Some("посади"));
        assert_eq!(landing("ПОСАДИТ!").as_deref(), Some("ПОСАДИТ"));
        assert_eq!(landing("по-посадке").as_deref(), Some("посадке"));
        assert_eq!(landing("сажай-ка").as_deref(), Some("сажай"));
        assert_eq!(landing("пересадить"), None);
        assert_eq!(landing("_посадка"), None);
        assert_eq!(landing("саж"), None);
        assert_eq!(folder("Живая  папка").as_deref(), Some("Живая  папка"));
        assert_eq!(folder("живой\nпапкой").as_deref(), Some("живой\nпапкой"));
        assert_eq!(folder("живое папка"), None);
    }

    #[test]
    fn first_person_and_quotes_are_read_as_python_reads_them() {
        let found = |text: &str| first_person(&chars(text));
        assert_eq!(found("Я тут").as_deref(), Some("Я"));
        assert_eq!(found("МОЁ дело").as_deref(), Some("МОЁ"));
        assert_eq!(found("меня.").as_deref(), Some("меня"));
        assert_eq!(found("моими словами").as_deref(), Some("моими"));
        assert_eq!(found("мой-то"), None);
        assert_eq!(found("по-мой"), None);
        assert_eq!(found("мои_"), None);
        assert_eq!(found("тебя"), None);
        assert_eq!(unquoted("a «b» c \"d\" „e“ «f"), "a   c     «f");
    }

    #[test]
    fn an_item_keeps_the_number_it_begins_with() {
        let number = |text: &str| item_number(text);
        assert_eq!(number("3. текст"), Some(("3".into(), "текст".into())));
        assert_eq!(number("4a) x"), Some(("4a".into(), "x".into())));
        assert_eq!(number("12б. да"), Some(("12б".into(), "да".into())));
        assert_eq!(number("007. y"), Some(("007".into(), "y".into())));
        assert_eq!(number("4A. x"), None);
        assert_eq!(number("1.x"), None);
        assert_eq!(number("1ё. x"), None);
        assert_eq!(next_number("9"), "10");
        assert_eq!(larger("007", "12"), "12");
        assert_eq!(next_number(&larger("99999999999999999999999", "3")), "100000000000000000000000");
    }

    #[test]
    fn a_page_too_wide_for_the_phone_is_seen() {
        let drawn = |wide: &str| format!("has words inside a drawing {wide} points wide. In the app on a phone the page is about \
                                          290 points wide, and a drawing shrinks to that with its words, past reading. Write the words as words of the page and keep \
                                          a drawing to shapes; build a scheme of the kit's chain (skills/hitl/pages/scheme.html), \
                                          which stands in a row in the window and one under another on the phone.");
        assert_eq!(narrow_trouble("<svg viewBox=\"0 0 600 100\"><text>x</text></svg>"), drawn("600"));
        assert_eq!(narrow_trouble("<svg width=\"400\" height=\"10\"><text>x</text></svg>"), drawn("400"));
        assert_eq!(narrow_trouble("<svg viewBox=\"0 0 200 100\" width=\"900\"><text>x</text></svg>"), "");
        assert_eq!(narrow_trouble("<svg viewBox=\"0 0 900 100\"><rect/></svg>"), "");
        assert_eq!(narrow_trouble("<style>@media (min-width: 600px) { a { max-width: 900px } }</style>"), "");
        assert!(narrow_trouble("<style>a { MIN-WIDTH : 320.5PX }</style>").starts_with("sets «MIN-WIDTH: 320.5px». In the app"));
        assert!(narrow_trouble("<div style=\"width:301px\">x</div>").starts_with("sets «width: 301px»."));
        assert_eq!(narrow_trouble("<div style=\"width:300px\">x</div><svg viewBox=\"0,0,350.7,1\" stroke-width=\"9\"><TEXT/></svg>"),
                   drawn("350"));
    }

    #[test]
    fn a_check_is_read_with_every_option() {
        let parsed = parse(&with_base(&["--kind", "question", "--check", "3. Первый → видно", "--details", " строка 1 \n\n  строка   2 ",
                                        "--fact", "факт **один**", "--link", "https://x.y", "--copy", " echo 1 ", "--option", "Да",
                                        "--option", "Нет", "--many", "--as", "plan", "--check", "второй", "--as", "question",
                                        "--check", "[team/320](https://a.b) задача", "--tag", " r/7 ", "--no-wait"]), true).unwrap();
        assert_eq!(parsed, json!({
            "title": "Вход", "about": "Плагин входит без пароля.", "kind": "question",
            "items": [{"label": "3", "text": "Первый → видно", "details": "строка 1\nстрока 2", "facts": ["факт **один**"],
                       "links": ["https://x.y"], "copies": ["echo 1"], "options": ["Да", "Нет"], "many": true, "kind": "plan",
                       "kindTitle": "план", "answers": ["да", "нет"]},
                      {"label": "4", "text": "второй"},
                      {"label": "5", "text": "[team/320](https://a.b) задача"}],
            "tag": "r/7", "wait": false, "after": null}));
        let keys: Vec<&String> = parsed["items"][0].as_object().unwrap().keys().collect();
        assert_eq!(keys, ["label", "text", "details", "facts", "links", "copies", "options", "many", "kind", "kindTitle", "answers"]);
    }

    #[test]
    fn a_call_out_of_bounds_is_refused_in_pythons_words() {
        assert_eq!(refused(&["--check"]), "--check needs a value.");
        assert_eq!(refused(&["--fact", "x"]), "--fact goes after the --check it belongs to.");
        // The command's name is the edition's, and other tests turn the edition: only what does not change is held.
        let bogus = refused(&["--bogus"]);
        assert!(["unknown option --bogus. Run hitl check --help.", "unknown option --bogus. Run hitl check --help."].contains(&bogus.as_str()));
        assert_eq!(refused(&["--title", "x"]), "nothing to check: give at least one --check \"что сделать → что должно быть\".");
        assert_eq!(refused(&["--check", "x"]), "--title says in a few words what the check is about.");
        assert_eq!(refused(&["--title", "t", "--check", "x"]), ABOUT);
        assert_eq!(refused(&["--kind", "nope"]), "--kind is one of: checklist, result, plan, question, action, explain.");
        assert_eq!(refused(&["--check", "a", "--as", "nope"]), "--as is one of: checklist, plan, question, action, explain.");
        assert_eq!(refused(&["--title", "[a](https://b)"]), "--title has a link in it. The title names the check, in the queue and \
                                                            in a notification too. A link goes into the text of an item, its \
                                                            --details or a --fact.");
        assert_eq!(refused(&["--check", "[a](b c)"]), "item 1: «[a](b c)» reads as a link, and «b c» is nothing to open. A link in a \
                                                      text is [слова](адрес), where the address is a page (https://…), another app's \
                                                      own address (obsidian://…) or the full path of a file on this Mac (/Users/… or ~/…).");
        assert_eq!(refused(&["--check", "[a](/no/such/file)"]), "item 1: the link «a» leads to /no/such/file, and there is no such file.");
        assert_eq!(refused(&["--check", &"x".repeat(121)]), format!("item 1 is 121 characters long, the limit is 120. {SPLIT}"));
        // An item is named by the place of the first item equal to it, as Python's list.index finds it.
        assert_eq!(refused(&["--check", "a", "--copy", "x", "--check", "a", "--copy", "x", "--copy", "x"]),
                   "item 1 gives «x» to copy twice.");
        assert_eq!(refused(&with_base(&["--check", "a", "--many"]).iter().map(String::as_str).collect::<Vec<_>>()),
                   "item 1 has --many and no options: --many says that several of the item's own --option may hold at once.");
        assert_eq!(refused(&with_base(&["--check", "a", "--option", "x"]).iter().map(String::as_str).collect::<Vec<_>>()),
                   "item 1 offers 1 options: a question has 2 to 9 answers to choose from. A yes or no is --kind question without options.");
        assert_eq!(refused(&with_base(&["--check", "2. a", "--check", "b", "--check", "3. c"]).iter().map(String::as_str).collect::<Vec<_>>()),
                   "two items share the number 3.");
        assert_eq!(refused(&with_base(&["--kind", "result", "--check", "a", "--as", "plan"]).iter().map(String::as_str).collect::<Vec<_>>()),
                   "item 1 has --as in a result: an «ок» to a result is the word for a merge, so a result holds nothing else. Put the \
                    question or the action into a check of its own.");
    }

    #[test]
    fn his_own_rules_hold_on_conveyor_alone() {
        let words = with_base(&["--check", "Посадка ветки"]);
        let found = parse(&words, true).expect_err("refused");
        assert!(found.starts_with("item 1: «Посадка» is a word he does not understand (неясно, имеется ли в виду слияние ветки): say «влить в main»."));
        assert!(parse(&words, false).is_ok());
        assert!(parse(&with_base(&["--check", "«посадка» его слово"]), true).is_ok());
        let explained = parse(&with_base(&["--kind", "explain", "--check", "Это", "--fact", "я проверил"]), false).expect_err("refused");
        assert!(explained.starts_with("a fact of item 1: «я» in an explanation."));
        assert!(parse(&with_base(&["--check", "я проверил"]), false).is_ok());
    }

    #[test]
    fn items_without_a_number_continue_the_count() {
        let parsed = parse(&with_base(&["--check", "a", "--check", "5. b", "--check", "c", "--check", "2a. d", "--check", "e"]), false).unwrap();
        let labels: Vec<&str> = parsed["items"].as_array().unwrap().iter().map(|item| item["label"].as_str().unwrap()).collect();
        assert_eq!(labels, ["1", "5", "6", "2a", "7"]);
    }

    #[test]
    fn a_card_is_found_by_its_number_alone() {
        let root = std::env::temp_dir().join(format!("hitl-cards-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("a/b")).unwrap();
        std::fs::write(root.join("a/b/Note.md"), "\u{feff}---\r\ntitle: x\r\nnumber: \"team/42\" # c\r\n---\r\nnumber: team/7\n").unwrap();
        std::fs::write(root.join("open.md"), "---\nnumber: team/9\n").unwrap();
        std::fs::write(root.join("twice.md"), "---\nnumber: team/8\nnumber: team/8\n---\n").unwrap();
        std::fs::write(root.join("bad.md"), "---\nnumber: team/08\n---\n").unwrap();
        assert_eq!(note_number(&root.join("a/b/Note.md")).as_deref(), Some("team/42"));
        assert_eq!(note_number(&root.join("open.md")), None);
        assert_eq!(note_number(&root.join("twice.md")), None);
        assert_eq!(note_number(&root.join("bad.md")), None);
        assert_eq!(card_in(" R/042 ", &root), Some(json!({"title": "карточка 42", "path": root.join("a/b/Note.md").display().to_string()})));
        assert_eq!(card_in("team/9", &root), None);
        assert_eq!(card_in("x/42", &root), None);
        std::fs::write(root.join("again.md"), "---\nnumber: 'team/42'\n---\n").unwrap();
        assert_eq!(card_in("team/42", &root), None);
        let _ = std::fs::remove_dir_all(&root);
    }
}
