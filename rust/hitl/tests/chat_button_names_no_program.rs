//! hitl is shared with the people who install it apart from hitl, and it must not know how any program opens
//! its chats (his word, 2026-11-04: «hitl который дистрибутится отдельно не должен знать эти тонкости. кнопка чата
//! должна быть настраиваемой без специфических знаний помещённых в код hitl»). That knowledge is the setting's
//! (`chat-button.json`), written by hitl. This reads what the «Чат» button is made of, in hitl and in the window,
//! and finds no program's name in it.

use std::path::Path;

const NAMES: [&str; 7] = ["codex", "opencode", "claude", "iterm", "conveyor-chat", "resume", "--session"];

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// The text from `start` to the first line `end` after it, or to the end of the file.
fn part(file: &str, start: &str, end: &str) -> String {
    let text = std::fs::read_to_string(root().join(file)).unwrap_or_else(|error| panic!("{file}: {error}"));
    let from = text.find(start).unwrap_or_else(|| panic!("{file}: no {start:?}"));
    let rest = &text[from..];
    rest[..rest.find(end).map(|at| at + end.len()).unwrap_or(rest.len())].to_string()
}

fn names_in(label: &str, text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    NAMES.iter().filter(|name| lower.contains(*name)).map(|name| format!("{label}: {name}")).collect()
}

#[test]
fn what_the_chat_button_is_made_of_names_no_program() {
    let mut found = Vec::new();
    let whole_file = |file: &str| std::fs::read_to_string(root().join(file)).unwrap();
    // The module of the setting, without its tests (which name made-up programs only, but are not shipped).
    let module = whole_file("src/chatbutton.rs");
    found.extend(names_in("src/chatbutton.rs", &module));
    found.extend(names_in("common::chat_link", &part("src/common.rs", "pub fn chat_link", "\n}\n")));
    found.extend(names_in("names::open_link", &part("src/names.rs", "pub fn open_link", "\n}\n")));
    found.extend(names_in("window/ChatButton.swift", &whole_file("../../window/ChatButton.swift")));
    found.extend(names_in("chatReference", &part("../../window/CheckNames.swift", "func chatReference", "\n    }\n")));
    found.extend(names_in("open(address:)", &part("../../window/Controller+Links.swift", "func open(address", "\n    }\n")));
    found.extend(names_in("run(command:)", &part("../../window/Controller+Links.swift", "private func run(command", "\n    }\n")));
    assert!(found.is_empty(), "the button names a program: {found:#?}");
}
