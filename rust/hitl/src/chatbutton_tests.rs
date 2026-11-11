use super::*;

fn env_with(pairs: &[(&str, &str)]) -> Env {
    pairs.iter().map(|(name, value)| (name.to_string(), value.to_string())).collect()
}

/// A settings file in a folder of its own, and the environment that names it.
fn world(name: &str, text: &str) -> Env {
    let folder = std::env::temp_dir().join(format!("hitl-chatbutton-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let file = folder.join("chat-button.json");
    std::fs::write(&file, text).unwrap();
    env_with(&[("CONVEYOR_CHAT_BUTTON", file.to_str().unwrap()), ("PATH", "/usr/bin:/bin")])
}

fn shell(script: &str) -> String {
    serde_json::json!({"link": ["/bin/sh", "-c", script]}).to_string()
}

#[test]
fn the_command_in_the_settings_finds_the_link_with_the_chat_s_environment() {
    let environment = { let mut e = world("finds", &shell("echo \"my-chat://${CHAT_APP}/${CHAT_ID}\"")); e.insert("CHAT_APP".into(), "Some".into()); e.insert("CHAT_ID".into(), "id-1".into()); e };
    assert_eq!(link_of(&environment), "my-chat://Some/id-1");
}

#[test]
fn no_file_a_failed_command_a_slow_one_and_a_non_address_give_no_button() {
    assert_eq!(link_of(&env_with(&[("CONVEYOR_CHAT_BUTTON", "/nonexistent/chat-button.json")])), "");
    assert_eq!(link_of(&world("garbage", "not json at all")), "");
    assert_eq!(link_of(&world("failed", &shell("echo my-chat://a/b; exit 3"))), "");
    assert_eq!(link_of(&world("words", &shell("echo two words"))), "");
    assert_eq!(link_of(&world("silent", &shell("true"))), "");
    assert_eq!(link_of(&world("missing-program", &serde_json::json!({"link": ["/nonexistent/program"]}).to_string())), "");
    let started = std::time::Instant::now();
    assert_eq!(link_of(&world("slow", &shell("sleep 30; echo my-chat://a/b"))), "");
    assert!(started.elapsed().as_secs_f64() < 10.0, "a check waits for the command no longer than a few seconds");
}

#[test]
fn a_link_named_by_hand_wins_and_a_chat_named_by_hand_has_none() {
    let mut named = world("named", &shell("echo my-chat://a/b"));
    named.insert("CONVEYOR_CHAT_LINK".into(), " claude://x/y ".into());
    assert_eq!(link_of(&named), "claude://x/y");
    let mut chat = world("chat", &shell("echo my-chat://a/b"));
    chat.insert("CONVEYOR_CHAT".into(), "named by hand".into());
    assert_eq!(link_of(&chat), "");
}

#[test]
fn the_settings_are_read_whole_or_not_at_all() {
    let settings = parse(r#"{"link": ["/c", "chat", "link"], "open": {"My-Chat": ["/c", "show", "{host}", "{path}"], "bad": [], "worse": ["", "x"]}}"#);
    assert_eq!(settings.link, ["/c", "chat", "link"]);
    assert_eq!(settings.open.keys().collect::<Vec<_>>(), ["my-chat"]);
    assert_eq!(parse("[1, 2]"), Settings::default());
    assert_eq!(parse(r#"{"link": ["/c", 3]}"#).link, ["/c"]); // a word that is no text is left out, the rest stands
    assert_eq!(parse(r#"{"link": ["/c", ""]}"#).link, Vec::<String>::new());
}

#[test]
fn the_button_runs_what_the_settings_say_for_the_scheme_of_the_address() {
    let settings = parse(r#"{"open": {"my-chat": ["/c", "show", "{host}", "{path}", "{scheme}", "{link}"]}}"#);
    assert_eq!(open_command(&settings, "my-chat://Some/id-1"), Some(["/c", "show", "Some", "id-1", "my-chat", "my-chat://Some/id-1"].map(String::from).to_vec()));
    assert_eq!(open_command(&settings, "MY-CHAT://Some/id-1").map(|c| c[2].clone()), Some("Some".into())); // a scheme has no case
    assert_eq!(open_command(&settings, "claude://code/session_1"), None); // opens as any address
    assert_eq!(open_command(&settings, "/Users/x/file.md"), None);
    assert_eq!(open_command(&settings, "my-chat:"), None);
    assert_eq!(open_command(&settings, "https://example.com/a"), None);
    assert_eq!(open_command(&Settings::default(), "my-chat://Some/id-1"), None);
}

#[test]
fn an_address_that_needs_a_command_of_the_mac_is_told_from_the_file_alone() {
    let environment = world("phone", r#"{"open": {"my-chat": ["/c"]}}"#);
    assert!(needs_command(&environment, "my-chat://a/b"));
    assert!(!needs_command(&environment, "claude://code/session_1"));
    assert!(!needs_command(&env_with(&[("CONVEYOR_CHAT_BUTTON", "/nonexistent")]), "my-chat://a/b"));
}
