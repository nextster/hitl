//! `hitl check`, `queue`, `wait` and `drop` as a chat calls them: the port of the class `CheckTests` of
//! `tests/check-tests.py`. What the command prints and exits with, the ticket it queues, the verdict it prints.
//! The verdicts are written the way the window writes them; no window is ever started.

mod common;

use common::{ABOUT, Answers, CHAT, World, contains, git, lacks, read};
use serde_json::{Value, json};
use std::path::Path;

fn path(place: &Path) -> String {
    place.display().to_string()
}

/// `urllib.parse.quote(text, safe="")`.
fn quoted(text: &str) -> String {
    text.bytes().map(|byte| if byte.is_ascii_alphanumeric() || b"_.-~".contains(&byte) { (byte as char).to_string() } else { format!("%{byte:02X}") })
        .collect()
}

/// Each call is refused with 64 and says this.
fn refused(world: &World, cases: &[(Vec<&str>, &str)]) {
    for (arguments, said) in cases {
        let done = world.check(arguments);
        assert_eq!(done.code, 64, "{arguments:?}: {}", done.err);
        contains(&done.err, said);
    }
}

#[test]
fn a_check_needs_a_title_and_an_item_and_modifiers_need_their_item() {
    let world = World::new("needs");
    for arguments in [&["--title", "Тема"][..], &["--check", "1. Пункт"], &["--title", "Тема", "--details", "x", "--check", "1. Пункт"],
                      &["--title", "Тема", "--check", "1. Раз", "--check", "1. Два"]] {
        let done = world.check(arguments);
        assert_eq!(done.code, 64, "{arguments:?}");
        contains(&done.err, "hitl check:");
    }
}

#[test]
fn a_check_without_a_word_about_the_task_is_refused() {
    let world = World::new("about");
    let done = world.check_bare(&["--title", "Вход", "--check", "1. Влить?", "--no-wait"]);
    assert_eq!(done.code, 64);
    contains(&done.err, "does not remember your task");
    let long = "длинно ".repeat(40);
    assert_eq!(world.check(&["--title", "Вход", "--about", &long, "--check", "1. Влить?", "--no-wait"]).code, 64);
    assert!(world.queued().is_empty());
    let ticket = world.ticket(&world.check(&["--title", "Вход", "--check", "1. Влить?", "--no-wait"]).id());
    assert_eq!((ticket["about"].clone(), ticket["card"].clone()), (json!(ABOUT), Value::Null)); // no card for r/66 on this made-up board
    assert_eq!(ticket["chatLink"], ""); // and a chat named by hand has no address to open it by
    let done = world.check_with(&["--title", "Вход", "--check", "1. Влить?", "--no-wait"],
                                &[("CONVEYOR_CHAT_LINK", Some("claude://code/session_01AbC9"))]);
    assert_eq!(world.ticket(&done.id())["chatLink"], "claude://code/session_01AbC9");
}

#[test]
fn a_check_carries_the_address_the_setting_of_the_chat_button_finds() {
    let world = World::new("chat-button");
    // A chat that is not named by hand: its address is asked of the command the setting names, with its environment.
    let setting = world.root.join("chat-button.json");
    std::fs::write(&setting, json!({"link": ["/bin/sh", "-c", "echo my-chat://${FAKE_APP}/${FAKE_ID}"]}).to_string()).unwrap();
    let setting_text = setting.display().to_string();
    let asked = |extra: &[(&str, Option<&str>)]| {
        let mut changes = vec![("CONVEYOR_CHAT", None), ("CONVEYOR_CHAT_BUTTON", Some(setting_text.as_str()))];
        changes.extend_from_slice(extra);
        world.ticket(&world.check_with(&["--title", "Вход", "--check", "1. Влить?", "--no-wait"], &changes).id())["chatLink"].clone()
    };
    assert_eq!(asked(&[("FAKE_APP", Some("Some")), ("FAKE_ID", Some("id-1"))]), "my-chat://Some/id-1");
    // No file, or a command that has nothing to say: no button, and the check is queued all the same.
    assert_eq!(world.ticket(&world.check_with(&["--title", "Вход", "--check", "1. Влить?", "--no-wait"],
                                              &[("CONVEYOR_CHAT", None), ("CONVEYOR_CHAT_BUTTON", Some("/nonexistent/chat-button.json"))]).id())["chatLink"], "");
    std::fs::write(&setting, json!({"link": ["/bin/sh", "-c", "exit 1"]}).to_string()).unwrap();
    assert_eq!(asked(&[]), "");
}

#[test]
fn a_link_too_long_to_be_a_file_s_name_is_still_a_link() {
    let world = World::new("long-link");
    // A link that opens a new chat of Codex with the request in it: 1100 characters, and the command fell on it.
    let address = format!("codex://threads/new?prompt={}", quoted(&"Проверка связи с hitl. ".repeat(8)));
    assert!(address.len() > 255); // more than a file's name may hold
    let done = world.check(&["--title", "Вход", "--check", "1. Нажми кнопку", "--link", &address, "--no-wait"]);
    assert_eq!(done.code, 75, "{}", done.err);
    assert_eq!(world.ticket(&done.id())["items"][0]["links"], json!([address]));
    // The same name is no picture, no page and no file of details: each is refused in words, not by a fall.
    refused(&world, &[(vec!["--title", "Вход", "--check", "1. Нажми кнопку", "--image", &address], "--image: there is no file"),
                      (vec!["--title", "Вход", "--check", "1. Нажми кнопку", "--page", &address], "--page: there is no file")]);
    let details = format!("@{}", "я".repeat(150));
    assert_eq!(world.check(&["--title", "Вход", "--check", "1. Пункт", "--details", &details, "--no-wait"]).code, 75);
}

/// The card a check of this tag carries.
fn card_of(world: &World, tag: &str) -> Value {
    let done = world.check(&["--title", "Вход", "--check", "1. Влить?", "--tag", tag, "--no-wait"]);
    assert_eq!(done.code, 75, "{}", done.err);
    world.ticket(&done.id())["card"].clone()
}

#[test]
fn task_note_follows_number_after_rename_and_topic_move() {
    let world = World::new("note-moves");
    let root = world.home.join("vault/cards");
    let note = root.join("Тема/Совсем другое имя.md");
    std::fs::create_dir_all(note.parent().unwrap()).unwrap();
    std::fs::write(&note, "---\nnumber: 'team/354' # task identity\n---\n").unwrap();
    // An old pointer and a filename that happens to begin with the number are not the task.
    std::fs::write(root.join("354-old.md"), "---\nmoved_to: https://realize.day/r/team/354\n---\nnumber: team/354\n").unwrap();
    for tag in ["team/354", "r/354", "TEAM/354"] {
        assert_eq!(card_of(&world, tag), json!({"title": "карточка 354", "path": path(&note)}), "{tag}");
    }
    let moved = root.join("Выполнено/Новое название.md");
    std::fs::create_dir_all(moved.parent().unwrap()).unwrap();
    std::fs::rename(&note, &moved).unwrap();
    assert_eq!(card_of(&world, "team/354")["path"], path(&moved));
    std::fs::remove_file(&moved).unwrap();
    assert_eq!(card_of(&world, "r/354"), Value::Null); // no fallback to a stale pointer
}

#[test]
fn a_picture_is_taken_by_its_file_and_a_wrong_one_is_refused() {
    let world = World::new("picture");
    let picture = world.file("вид.png", b"\x89PNG\r\n\x1a\n");
    let text = world.file("заметка.md", "x");
    let done = world.check(&["--title", "Тема", "--check", "1. Так?", "--image", &path(&picture), "--link", &path(&picture),
                             "--link", &path(&text), "--no-wait"]);
    assert_eq!(done.code, 75, "{}", done.err);
    let item = &world.ticket(&done.id())["items"][0];
    let resolved = path(&picture.canonicalize().unwrap());
    assert_eq!((item["images"].clone(), item["links"].clone()), (json!([resolved, resolved]), json!([path(&text)])));
    let missing = path(&world.root.join("нет.png"));
    let five: Vec<&str> = std::iter::repeat(["--image", picture.to_str().unwrap()]).take(5).flatten().collect();
    let mut many = vec!["--title", "Тема", "--check", "1. Так?"];
    many.extend(five);
    refused(&world, &[(vec!["--title", "Тема", "--check", "1. Так?", "--image", &missing], "there is no file"),
                      (vec!["--title", "Тема", "--check", "1. Так?", "--image", &path(&text)], "takes a picture"),
                      (many, "more than 4 pictures")]);
}

#[test]
fn a_line_to_copy_goes_as_it_is_typed_and_is_one_line() {
    let world = World::new("copy");
    let long = "x".repeat(301);
    let mut four = vec!["--title", "Тема", "--check", "1. Пункт"];
    let lines: Vec<String> = (0..4).map(|number| format!("ls {number}")).collect();
    for line in &lines {
        four.extend(["--copy", line.as_str()]);
    }
    refused(&world, &[(vec!["--title", "Тема", "--copy", "ls", "--check", "1. Пункт"], "goes after the --check"),
                      (vec!["--title", "Тема", "--check", "1. Пункт", "--copy", "раз\nдва"], "has several lines"),
                      (vec!["--title", "Тема", "--check", "1. Пункт", "--copy", &long], "the limit is 300"),
                      (vec!["--title", "Тема", "--check", "1. Пункт", "--copy", "ls", "--copy", " ls"], "to copy twice"),
                      (four, "more than 3 lines"),
                      (vec!["--title", "Тема", "--check", "1. Пункт", "--copy", "  "], "--copy needs a text")]);
    assert!(world.queued().is_empty());
    let command = "/Users/example/Code/scripts/hitl phone relay up";
    let padded = format!("  {command} ");
    let done = world.check(&["--title", "Сервер", "--kind", "explain", "--check", "1. Сервер поднимает команда в терминале",
                             "--copy", &padded, "--copy", "echo  'я посадил'", "--check", "2. Без строк", "--no-wait"]);
    assert_eq!(done.code, 75, "{}", done.err);
    let ticket = world.ticket(&done.id());
    // The spaces inside stay, and what is copied is not prose: no word of it is looked at.
    assert_eq!(ticket["items"][0]["copies"], json!([command, "echo  'я посадил'"]));
    assert!(ticket["items"][1].get("copies").is_none());
}

#[test]
fn facts_are_short_lines_under_an_item_and_too_many_are_refused() {
    let world = World::new("facts");
    let long = "х".repeat(101);
    let mut six = vec!["--title", "Тема", "--check", "1. Пункт"];
    for _ in 0..6 {
        six.extend(["--fact", "раз"]);
    }
    refused(&world, &[(vec!["--title", "Тема", "--check", "1. Пункт", "--fact", &long], "a fact is one line"),
                      (six, "more than 5 facts"),
                      (vec!["--title", "Тема", "--check", "1. Пункт", "--fact", "  "], "--fact needs a text"),
                      (vec!["--title", "Тема", "--fact", "раз", "--check", "1. Пункт"], "goes after the --check")]);
    assert!(world.queued().is_empty());
    let done = world.check(&["--title", "Тема", "--check", "1. Убрать уведомление?", "--fact", "Шлёт  **сам hitl merge**,   в конце вливания",
                             "--fact", "Выключателя нет", "--option", "Убрать", "--option", "Оставить", "--check", "2. Без фактов", "--no-wait"]);
    assert_eq!(done.code, 75, "{}", done.err);
    let ticket = world.ticket(&done.id());
    assert_eq!(ticket["items"][0]["facts"], json!(["Шлёт **сам hitl merge**, в конце вливания", "Выключателя нет"]));
    assert!(ticket["items"][1].get("facts").is_none());
}

#[test]
fn an_explanation_is_answered_as_understood_or_not() {
    let world = World::new("explain");
    let done = world.check(&["--title", "Откуда уведомление", "--kind", "explain", "--check", "1. Шлёт сам hitl merge",
                             "--fact", "В конце каждого вливания", "--check", "2. Выключателя для него одного нет", "--no-wait"]);
    let id = done.id();
    let ticket = world.ticket(&id);
    assert_eq!((ticket["kindTitle"].clone(), ticket["answers"].clone(), ticket["items"][0]["facts"].clone()),
               (json!("объяснение"), json!(["понятно", "не понятно"]), json!(["В конце каждого вливания"])));
    contains(&world.run(&["queue"]).out, "объяснение");
    world.finish_as(&id, &[Some("ok"), Some("problem")], Answers { notes: &[("2", "а как тогда выключить")], ..Default::default() });
    let verdict = world.run(&["wait", &id]).out;
    contains(&verdict, "Понятно: 1");
    contains(&verdict, "Не понятно: 2");
    contains(&verdict, "2. а как тогда выключить");
    let id = world.check(&["--title", "Откуда уведомление", "--kind", "explain", "--check", "1. Шлёт сам hitl merge", "--no-wait"]).id();
    world.finish(&id, &[Some("ok")]);
    contains(&world.run(&["wait", &id]).out, "Всё понятно, и закончил проверку сам Проверяющий.");
}

#[test]
fn a_page_is_one_small_html_file_an_item() {
    let world = World::new("page");
    let page = path(&world.file("страница.html", "<p>страница</p>"));
    let text = path(&world.file("заметка.md", "x"));
    let heavy = path(&world.file("тяжёлая.html", "x".repeat(300_000 + 1)));
    let missing = path(&world.root.join("нет.html"));
    refused(&world, &[(vec!["--title", "Тема", "--check", "1. Так?", "--page", &missing], "there is no file"),
                      (vec!["--title", "Тема", "--check", "1. Так?", "--page", &text], "takes an HTML file"),
                      (vec!["--title", "Тема", "--check", "1. Так?", "--page", &page, "--page", &page], "one page an item"),
                      (vec!["--title", "Тема", "--check", "1. Так?", "--page", &heavy], "the limit is 300 KB"),
                      (vec!["--title", "Тема", "--page", &page, "--check", "1. Так?"], "goes after the --check")]);
    let done = world.check(&["--title", "Тема", "--check", "1. Так?", "--page", &page, "--check", "2. Без страницы", "--no-wait"]);
    assert_eq!(done.code, 75, "{}", done.err);
    let ticket = world.ticket(&done.id());
    assert_eq!(ticket["items"][0]["page"], path(&Path::new(&page).canonicalize().unwrap()));
    assert!(ticket["items"][1].get("page").is_none());
}

#[test]
fn a_page_that_would_break_on_the_phone_is_refused() {
    // On his phone a page is half as wide as in the window: a scheme drawn with its words inside came back
    // as «херово адаптировано на мобилу».
    let world = World::new("narrow");
    let drawn = path(&world.file("схема.html", "<svg viewBox=\"0 0 580 150\"><rect width=\"80\" height=\"40\"/><text x=\"4\" y=\"20\">Чат</text></svg>"));
    let wide = path(&world.file("широкая.html", "<style>.box { width: 560px; } @media (min-width: 500px) { .box { color: red } }</style><div class=box>x</div>"));
    for (page, said) in [(&drawn, "words inside a drawing 580 points wide"), (&wide, "sets «width: 560px»")] {
        let done = world.check(&["--title", "Тема", "--check", "1. Так?", "--page", page]);
        assert_eq!(done.code, 64, "{page}");
        contains(&done.err, said);
        contains(&done.err, "290 points wide");
    }
    // A drawing without words, a small drawing with them, a width the page may be narrower than: all go.
    let fine = path(&world.file("годная.html", "<style>.box { max-width: 560px; width: 100%; } @media (max-width: 420px) { .box { width: 280px } }</style>\
                                                <svg viewBox=\"0 0 580 150\"><rect width=\"80\" height=\"40\"/></svg>\
                                                <svg viewBox=\"0 0 100 100\"><text x=\"4\" y=\"20\">К</text></svg>"));
    let done = world.check(&["--title", "Тема", "--check", "1. Так?", "--page", &fine, "--no-wait"]);
    assert_eq!(done.code, 75, "{}", done.err);
    // And so do the pages of the kit, every one.
    let kit = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/hitl/pages");
    let mut pages: Vec<_> = std::fs::read_dir(&kit).unwrap().flatten().map(|entry| entry.path())
        .filter(|page| page.extension().is_some_and(|extension| extension == "html")).collect();
    pages.sort();
    assert!(!pages.is_empty(), "{}", kit.display());
    for page in pages {
        let done = world.check(&["--title", "Тема", "--check", "1. Так?", "--page", &path(&page), "--no-wait"]);
        assert_eq!(done.code, 75, "{}: {}", page.display(), done.err);
    }
}

#[test]
fn several_options_at_once_come_back_as_a_list() {
    let world = World::new("many");
    let done = world.check(&["--title", "Тема", "--check", "1. Что берём?", "--option", "Сервер", "--many", "--option", "Плагин",
                             "--check", "2. А тут одно?", "--option", "А", "--option", "Б", "--no-wait"]);
    let ticket = world.ticket(&done.id());
    assert_eq!((ticket["items"][0].get("many"), ticket["items"][1].get("many")), (Some(&json!(true)), None));
    refused(&world, &[(vec!["--title", "Тема", "--check", "1. Что берём?", "--many"], "--many and no options")]);
    let id = world.check(&["--title", "Плагин", "--kind", "question", "--check", "1. Что берём?", "--option", "Сервер", "--option", "Плагин",
                           "--option", "Документация", "--many", "--check", "2. А ещё?", "--option", "А", "--option", "Б", "--many",
                           "--check", "3. Одно?", "--option", "А", "--option", "Б", "--no-wait"]).id();
    world.finish_as(&id, &[Some("ok"), Some("ok"), Some("ok")],
                    Answers { choices: &[("3", "Б")], notes: &[("2", "ещё тесты")], ..Default::default() });
    world.change_verdict(&id, |verdict| {
        verdict["results"][0]["choices"] = json!(["Сервер", "Документация"]);
        verdict["results"][1]["other"] = json!(true);
    });
    let text = world.run(&["wait", &id]).out;
    contains(&text, "Выбор:\n1. Сервер; Документация\n2. своё, в комментарии\n3. Б");
    contains(&text, "2. ещё тесты");
    lacks(&text, "Да: ");
}

#[test]
fn no_wait_queues_the_check_under_the_chats_tag() {
    let world = World::new("no-wait");
    let done = world.check(&["--title", "Вход", "--kind", "result", "--check", "1. Влить?", "--no-wait"]);
    assert_eq!(done.code, 75);
    let ticket = world.ticket(&done.id());
    assert_eq!((ticket["tag"].clone(), ticket["chat"].clone(), ticket["kind"].clone()), (json!("r/66"), json!(CHAT), json!("result")));
    let listing = world.run(&["queue"]);
    assert_eq!(listing.code, 0);
    contains(&listing.out, "r/66 «Вход» · результат");
}

#[test]
fn only_a_check_ended_with_done_and_all_yes_is_green() {
    let world = World::new("green");
    let id = world.check(&["--title", "Результат", "--kind", "result", "--check", "1. Влить?", "--no-wait"]).id();
    world.finish(&id, &[Some("ok")]);
    let verdict = world.run(&["wait", &id]).out;
    contains(&verdict, "Ок: 1");
    contains(&verdict, "это его «ок»");
}

#[test]
fn drop_ends_a_waiting_check_without_a_verdict() {
    let world = World::new("drop");
    let id = world.check(&["--title", "Лишняя", "--check", "1. Пункт", "--no-wait"]).id();
    assert_eq!(world.run(&["drop", &id]).code, 0);
    let done = world.run(&["wait", &id]);
    assert_eq!(done.code, 2);
    contains(&done.out, "снята");
    assert!(world.queued().is_empty());
    // The history keeps what was asked and by which agent, of a check nobody looked at too.
    let kept = world.history().pop().unwrap();
    assert_eq!((kept["how"].clone(), kept["results"][0]["text"].clone(), kept.get("agent").is_some()), (json!("dropped"), json!("Пункт"), true));
}

#[test]
fn a_drop_does_not_take_the_place_of_the_verdict_the_window_wrote_first() {
    // The audit of 2026-10-16, H03: the other order of the same second — the window has linked his verdict in
    // and not yet taken the ticket away when the chat drops the check.
    let world = World::new("drop-late");
    let id = world.check(&["--title", "Лишняя", "--check", "1. Пункт", "--no-wait"]).id();
    let green = json!({"id": id, "kind": "checklist", "screen": "mac", "how": "done", "results": [{"label": "1", "state": "ok", "note": ""}]});
    std::fs::write(world.verdict_file(&id), green.to_string()).unwrap();
    let before = world.history().len();
    let done = world.run(&["drop", &id]);
    assert_eq!(done.code, 1);
    contains(&done.err, "уже закончил");
    assert_eq!(read(&world.verdict_file(&id))["how"], "done");
    assert_eq!(world.history().len(), before, "a «снята» went into the history beside his answer");
    let hidden: Vec<_> = std::fs::read_dir(world.home.join("done")).unwrap().flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with('.')).map(|entry| entry.path()).collect();
    assert!(hidden.is_empty(), "{hidden:?}");
}

#[test]
fn drop_preserves_saved_marks_comments_pictures_and_page_values() {
    // Which saved answers keep a check is held by queue.rs (saved_answers_keep_a_check_from_a_drop); here, that the
    // command refuses and leaves the ticket, the verdict and the saved answers as they were.
    let world = World::new("drop-saved");
    for mark in [json!({"state": "ok"}), json!({"note": "unfinished"}), json!({"shots": [{"file": "private.png"}]}), json!({"values": []})] {
        let ticket = world.check(&["--title", "Protected", "--check", "1. Item", "--no-wait"]).id();
        let mut full = json!({"state": null, "note": ""});
        for (key, value) in mark.as_object().unwrap() {
            full[key] = value.clone();
        }
        let saved = world.file("hitl/window/answers.json", json!({"checks": {ticket.clone(): {"marks": [full]}}, "active": ticket}).to_string());
        let before = std::fs::read(&saved).unwrap();
        assert_eq!(world.run(&["drop", &ticket]).code, 1, "{mark}");
        assert!(world.ticket_file(&ticket).exists());
        assert!(!world.verdict_file(&ticket).exists());
        assert_eq!(std::fs::read(&saved).unwrap(), before);
        std::fs::remove_file(&saved).unwrap();
    }
}

#[test]
fn a_word_he_does_not_understand_and_a_nameless_i_go_back_to_the_chat() {
    // Two rules his «не понял» made, each on his «да» (2026-10-11): a word he has tripped on is not passed to him,
    // and an explanation does not say «я».
    let world = World::new("words");
    for text in ["1. После посадки выложу на Development", "1. Сажать, не дожидаясь телефона?", "1. Файлы ставятся в живую папку"] {
        let done = world.check(&["--title", "Вход", "--check", text, "--no-wait"]);
        assert_eq!(done.code, 64, "{text}");
        contains(&done.err, "is a word he does not understand");
    }
    let done = world.check(&["--title", "Вход", "--check", "1. Готово", "--details", "Выложить — шаг после посадки", "--no-wait"]);
    assert_eq!(done.code, 64);
    contains(&done.err, "the details of item 1: «посадки»");
    contains(&done.err, "say «влить в main»");
    // His own words are his, and so is a word that only looks alike.
    for text in ["1. Ты спросил: «что такое посадка»", "1. Пассажир выходит на своей остановке"] {
        assert_eq!(world.check(&["--title", "Вход", "--check", text, "--no-wait"]).code, 75, "{text}");
    }
    let explained = ["--kind", "explain", "--title", "Почему не влито", "--check", "1. Приложение не знает, что ты нажал «Влить»"];
    let with = |more: &[&str]| {
        let mut arguments = explained.to_vec();
        arguments.extend_from_slice(more);
        arguments.push("--no-wait");
        world.check(&arguments)
    };
    let done = with(&["--fact", "Я это узнал, а защита нет"]);
    assert_eq!(done.code, 64);
    contains(&done.err, "a fact of item 1: «Я» in an explanation");
    contains(&done.err, "who exactly does what");
    assert_eq!(with(&["--details", "Ей видно только то, что ты написал мне в чате"]).code, 64);
    // What he said is quoted, an option is his own voice, and a plan may speak as the chat that will do the work.
    assert_eq!(with(&["--fact", "Ты спросил: «а \"я\" это кто»", "--option", "Понятно, мне хватит", "--option", "Нарисуй"]).code, 75);
    assert_eq!(world.check(&["--kind", "plan", "--title", "План", "--check", "1. Покажу тебе картинку, ты скажешь мне, годится ли", "--no-wait"]).code, 75);
}

#[test]
fn one_check_carries_an_explanation_a_question_and_a_step_of_his() {
    // «Можно миксовать вопросы и действия и объяснения в рамках одного набора вопросов»: an item may have a kind of
    // its own, with that kind's answers. (What the verdict of such a check says is held by verdict.rs.)
    let world = World::new("mixed");
    let done = world.check(&["--kind", "question", "--title", "Проверки GitHub стоят",
                             "--check", "1. GitHub остановил проверки: кончились оплаченные минуты", "--as", "explain", "--fact", "В тариф входит 2000 минут",
                             "--check", "2. Какой потолок поставить?", "--option", "20 $", "--option", "50 $",
                             "--check", "3. Открой оплату и поставь потолок → проверки пойдут снова", "--as", "action",
                             "--check", "4. Вернуть сборку приложения на каждое изменение?", "--as", "question", "--no-wait"]);
    assert_eq!(done.code, 75, "{}", done.err);
    let ticket = world.ticket(&done.id());
    let kinds: Vec<(Value, Value, Value)> = ticket["items"].as_array().unwrap().iter()
        .map(|item| (item.get("kind").cloned().unwrap_or(Value::Null), item.get("kindTitle").cloned().unwrap_or(Value::Null),
                     item.get("answers").cloned().unwrap_or(Value::Null))).collect();
    assert_eq!(kinds, [(json!("explain"), json!("объяснение"), json!(["понятно", "не понятно"])), (Value::Null, Value::Null, Value::Null),
                       (json!("action"), json!("действие"), json!(["сделал", "не сделаю", "позже"])), (Value::Null, Value::Null, Value::Null)]);
    // A result is a check of its own, and holds nothing else; a kind has to be one there is, and to belong to an item.
    for (wrong, said) in [(&["--title", "Т", "--check", "1. Влить?", "--as", "result"][..], "a result is a check of its own"),
                          (&["--title", "Т", "--kind", "result", "--check", "1. Открой оплату", "--as", "action"], "has --as in a result"),
                          (&["--title", "Т", "--check", "1. Шаг", "--as", "задача"], "--as is one of"),
                          (&["--title", "Т", "--as", "action", "--check", "1. Шаг"], "goes after the --check")] {
        let mut arguments = wrong.to_vec();
        arguments.push("--no-wait");
        let done = world.check(&arguments);
        assert_eq!(done.code, 64, "{wrong:?}");
        contains(&done.err, said);
    }
    // An explanation among other items is held to the explanation's rule: who does what is said by name.
    let done = world.check(&["--kind", "plan", "--title", "Т", "--check", "1. Я это узнал, а защита нет", "--as", "explain", "--no-wait"]);
    assert_eq!(done.code, 64);
    contains(&done.err, "in an explanation");
}

#[test]
fn a_check_says_which_of_his_projects_it_is_about() {
    // «Мне очень надо понять к чему конкретно относится текущий запрос»: the name he knows a project under is the
    // folder of the repository itself, whichever copy of it the chat works in.
    let world = World::new("project");
    let repository = world.root.join("Code/realize");
    std::fs::create_dir_all(repository.join("apps/ios")).unwrap();
    let copy = world.root.join("copies/nervous-nightingale");
    git(&world, &["-C", &path(&repository), "init", "-q"]);
    git(&world, &["-C", &path(&repository), "commit", "-q", "--allow-empty", "-m", "start"]);
    git(&world, &["-C", &path(&repository), "worktree", "add", "-q", &path(&copy), "-b", "work"]);
    let plain = world.root.join("Obsidian Vault");
    std::fs::create_dir_all(&plain).unwrap();
    let user = world.root.join("user");
    let named = |place: &Path| {
        let done = world.run_in(&["check", "--about", ABOUT, "--title", "Вход", "--check", "1. Пункт", "--no-wait"], &world.env, place);
        assert_eq!(done.code, 75, "{}", done.err);
        let name = world.ticket(&done.id())["projectName"].clone();
        assert_eq!(world.run_in(&["drop", &done.id()], &world.env, place).code, 0);
        name
    };
    let names: Vec<Value> = [&repository, &repository.join("apps/ios"), &copy, &plain, &user].iter().map(|place| named(place)).collect();
    assert_eq!(names, [json!("realize"), json!("realize"), json!("realize"), json!("Obsidian Vault"), json!("")]);
    // The check carries it to the window and the history, and the queue says it first.
    let done = world.run_in(&["check", "--about", ABOUT, "--title", "Вход", "--check", "1. Пункт", "--no-wait"], &world.env, &copy);
    assert_eq!(world.ticket(&done.id())["projectName"], "realize");
    contains(&world.run(&["queue"]).out, "Сейчас: realize · r/66 «Вход»");
    assert_eq!(world.run(&["drop", &done.id()]).code, 0);
    assert_eq!(world.history().pop().unwrap()["projectName"], "realize");
}

#[test]
fn an_item_asked_again_brings_his_last_answer_along() {
    // One question came to him four times in half an hour, each time as a new one. A chat that asks an item again
    // names the check it answers, and the item, under its old number, carries what he said to it then.
    let world = World::new("after");
    let first = world.check(&["--kind", "plan", "--title", "Номера задач", "--check", "5. Номер стоит в карточке", "--check", "6. Какой вид у ссылки?",
                              "--check", "7. Сайт открывает задачу по номеру", "--check", "8. Что ещё", "--no-wait"]).id();
    world.finish_as(&first, &[Some("ok"), Some("problem"), None, None],
                    Answers { how: "rework", notes: &[("6", "не понял что  и почему здесь"), ("8", "и ещё вот что")], ..Default::default() });
    contains(&world.run(&["wait", &first]).out, &format!("добавь к новой проверке --after {first} и оставь пунктам их номера"));
    let again = ["--kind", "question", "--title", "Короткая ссылка", "--after", first.as_str()];
    let asked = |items: &[&str]| {
        let mut arguments = again.to_vec();
        arguments.extend_from_slice(items);
        arguments.push("--no-wait");
        world.check(&arguments)
    };
    let done = asked(&["--check", "6. Какая ссылка короче?", "--check", "8. Ещё", "--check", "9. Новое"]);
    assert_eq!(done.code, 75, "{}", done.err);
    let ticket = world.ticket(&done.id());
    // Where he wrote or did not say yes: the answer in the words of its kind, "" when he only wrote.
    let before: Vec<Value> = ticket["items"].as_array().unwrap().iter().map(|item| item.get("before").cloned().unwrap_or(Value::Null)).collect();
    assert_eq!(before, [json!({"at": 2.0, "answer": "нет", "note": "не понял что и почему здесь"}),
                        json!({"at": 2.0, "answer": "", "note": "и ещё вот что"}), Value::Null]);
    // The number is how his answer finds the item; a number of no check is no check.
    let done = asked(&["--check", "5. Номер в карточке", "--check", "1. Другое"]);
    assert_eq!(done.code, 64);
    contains(&done.err, "none of these items has the number");
    contains(&done.err, "(6, 8)");
    let done = world.check(&["--title", "Т", "--after", "1011-000000-000-нет", "--check", "6. Вопрос", "--no-wait"]);
    assert_eq!(done.code, 64);
    contains(&done.err, "there is no ended check with that number");
    // A check he answered yes to all through is not asked again.
    let id = world.check(&["--kind", "plan", "--title", "План", "--check", "1. Шаг", "--no-wait"]).id();
    world.finish(&id, &[Some("ok")]);
    lacks(&world.run(&["wait", &id]).out, "--after");
}

#[test]
fn a_text_over_its_limit_is_refused_with_the_way_to_split_it() {
    let world = World::new("limits");
    let long_item = format!("1. {}", "Собираю каталог покупок из всех чеков за год, ".repeat(4));
    let long_details = "подробности ".repeat(30);
    let long_option = "вариант ".repeat(10);
    let long_title = "Тема ".repeat(20);
    let file = format!("@{}", path(&world.file("заметки.txt", "много текста")));
    refused(&world, &[(vec!["--title", "Тема", "--check", &long_item], "item 1 is"),
                      (vec!["--title", "Тема", "--check", "1. Пункт", "--details", &long_details], "the details of item 1"),
                      (vec!["--title", "Тема", "--check", "1. Пункт", "--details", "раз\nдва\nтри\nчетыре"], "in 4 lines"),
                      (vec!["--title", "Тема", "--check", "1. Куда?", "--option", "А", "--option", &long_option], "an option of item 1"),
                      (vec!["--title", "Тема", "--check", "1. Куда?", "--option", "А"], "offers 1 options"),
                      (vec!["--title", &long_title, "--check", "1. Пункт"], "--title is"),
                      (vec!["--title", "Тема", "--check", "1. Пункт", "--link", "a", "--link", "b", "--link", "c", "--link", "d"], "more than 3 links"),
                      (vec!["--title", "Тема", "--check", "1. Пункт", "--details", "раз", "--details", "два"], "already has"),
                      (vec!["--title", "Тема", "--check", "1. Пункт", "--details", &file], "does not read a file")]);
    assert!(world.queued().is_empty());
    let done = world.check(&["--title", "Тема", "--check", "1.  Пункт  с   пробелами", "--details", " раз \n\n два ", "--no-wait"]);
    let item = &world.ticket(&done.id())["items"][0];
    assert_eq!((item["text"].clone(), item["details"].clone()), (json!("Пункт с пробелами"), json!("раз\nдва")));
}

#[test]
fn a_link_in_a_text_is_read_by_its_words_and_takes_none_of_the_texts_room() {
    // «[team/320](адрес)» in a text of a check is «team/320» to him: the limits count what he reads, and a link that
    // would not open, or stands where a click answers or names, goes back to the chat. (How a text is read by its
    // words is held by common.rs, links_are_read_by_their_words.)
    let world = World::new("links");
    let address = format!("https://realize.day/t/team/320?{}", "x".repeat(300));
    let note = path(&world.file("Хранилище заметок/план работ.md", "план"));
    let about = format!("Правлю форму из задачи [team/320]({address}).");
    let text = format!("Форма после [team/320]({address}) закрывается рывком");
    let item = format!("1. {text}");
    let details = format!("Рывок описан в [плане]({note}), его видно на каждом телефоне.");
    let fact = "Правка в **одном** месте, [PR #412](https://github.com/o/r/pull/412) её не задевает";
    let done = world.check(&["--title", "Тема", "--about", &about, "--check", &item, "--details", &details, "--fact", fact, "--no-wait"]);
    assert_eq!(done.code, 75, "{}", done.err);
    let ticket = world.ticket(&done.id());
    // Kept as the chat wrote it: the window and the app on the phone make the link of it.
    assert_eq!((ticket["about"].clone(), ticket["items"][0]["text"].clone()), (json!(about), json!(text)));
    assert_eq!((ticket["items"][0]["details"].clone(), ticket["items"][0]["facts"].clone()), (json!(details), json!([fact])));
    let long_words = format!("1. [{}](https://example.test/1)", "очень длинные слова ссылки ".repeat(5));
    refused(&world, &[
        (vec!["--title", "Тема", "--check", &long_words], "item 1 is 134 characters long"),
        (vec!["--title", "Тема", "--check", "1. Форма после [team/320](team/320) закрывается"], "«team/320» is nothing to open"),
        (vec!["--title", "Тема", "--check", "1. Пункт", "--details", "См. [план](~/нет-такого-файла.md)"], "there is no such file"),
        (vec!["--title", "Тема", "--check", "1. Пункт", "--fact", "См. [план](план.md)"], "a fact of item 1: «[план](план.md)» reads as a link"),
        (vec!["--title", "Задача [team/320](https://example.test/1)", "--check", "1. Пункт"], "--title has a link in it"),
        (vec!["--title", "Тема", "--check", "1. Куда?", "--option", "В [team/320](https://example.test/1)", "--option", "Никуда"],
         "an option of item 1 has a link in it"),
        // A word he does not understand is one in the words of a link too; in its address it is not read.
        (vec!["--title", "Тема", "--check", "1. Смотри [план посадки](https://example.test/1)"], "«посадки» is a word he does not understand"),
    ]);
    assert_eq!(world.check(&["--title", "Тема", "--check", "1. Смотри [план](https://example.test/посадка)", "--no-wait"]).code, 75);
}

#[test]
fn a_ticket_carries_the_names_of_its_answers() {
    let world = World::new("answers");
    for (kind, title, answers) in [("checklist", "чек-лист", json!(["работает", "не работает", "пропустить"])), ("plan", "план", json!(["да", "нет"])),
                                   ("action", "действие", json!(["сделал", "не сделаю", "позже"])),
                                   ("question", "вопрос", json!(["да", "нет", "пропустить"]))] {
        let ticket = world.ticket(&world.check(&["--title", "Тема", "--kind", kind, "--check", "1. Пункт", "--no-wait"]).id());
        assert_eq!((ticket["answers"].clone(), ticket["kindTitle"].clone()), (answers, json!(title)), "{kind}");
    }
}
