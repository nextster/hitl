mod partial_sort;
mod py;
use hitl_compat_json::{self as json, Value};
use num_bigint::BigInt;
use py::{Num, Result, Text};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, HashMap, HashSet},
    path::Path,
};

pub const USAGE: &str = include_str!("usage.txt");
pub const REFUSAL: &str = "usage: hitl stats [--days N] [--comments] [--json]";
#[derive(Debug)]
pub struct Options {
    pub days: BigInt,
    pub comments: bool,
    pub json: bool,
}
pub enum Arguments {
    Help,
    Run(Options),
}
pub enum Error {
    Usage,
    Data(&'static str),
    Output,
}
impl From<&'static str> for Error {
    fn from(s: &'static str) -> Self {
        Self::Data(s)
    }
}
include!("digits.rs");
pub fn arguments(args: &[String]) -> std::result::Result<Arguments, Error> {
    let mut options = Options {
        days: 7.into(),
        comments: false,
        json: false,
    };
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--help" || arg == "-h" {
            return Ok(Arguments::Help);
        }
        if arg == "--comments" {
            options.comments = true;
        } else if arg == "--json" {
            options.json = true;
        } else {
            let value = if arg == "--days" && i + 1 < args.len() {
                i += 1;
                &args[i]
            } else if let Some(s) = arg.strip_prefix("--days=") {
                s
            } else {
                return Err(Error::Usage);
            };
            if value.is_empty()
                || !value
                    .chars()
                    .all(|c| DIGITS.iter().any(|&(a, b)| (a..=b).contains(&u32::from(c))))
            {
                return Err(Error::Usage);
            }
            let digits = value
                .chars()
                .map(|c| {
                    DECIMALS.iter().find_map(|&a| {
                        let n = u32::from(c);
                        (a..a + 10)
                            .contains(&n)
                            .then(|| char::from(b'0' + (n - a) as u8))
                    })
                })
                .collect::<Option<String>>()
                .ok_or(Error::Data("invalid decimal integer"))?;
            if digits.trim_start_matches('0').len() > 4300 {
                return Err(Error::Data("integer exceeds decimal conversion limit"));
            }
            options.days = digits.parse().map_err(|_| Error::Data("invalid days"))?;
        }
        i += 1;
    }
    Ok(Arguments::Run(options))
}
fn since(days: &BigInt, now: f64) -> Result<Value> {
    if days == &BigInt::from(0) {
        return Ok(Value::Number("0".into()));
    }
    let seconds = Num::Int(days * 86400).float()?;
    Ok(Value::Number((now - seconds).to_string()))
}
fn split_lines(s: &str) -> impl Iterator<Item = &str> {
    // str.splitlines(), including separators other than LF; CRLF is one break.
    s.split([
        '\n', '\r', '\u{b}', '\u{c}', '\u{1c}', '\u{1d}', '\u{1e}', '\u{85}', '\u{2028}',
        '\u{2029}',
    ])
}
fn depth_limit() -> usize {
    std::env::var("CONVEYOR_STATS_JSON_DEPTH")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(989)
}
fn history(root: &Path, limit: usize) -> Result<Vec<Value>> {
    let bytes = match std::fs::read(root.join("history.jsonl")) {
        Ok(b) => b,
        Err(_) => return Ok(vec![]),
    };
    let data = std::str::from_utf8(&bytes).map_err(|_| "history is not UTF-8")?;
    let mut found = vec![];
    for line in split_lines(data) {
        match json::parse_with_limit(line, limit) {
            Ok(v @ Value::Object(_)) => found.push(v),
            Err(json::ParseError::Depth) => {
                return Err("history JSON nesting exceeds reader limit");
            }
            _ => {}
        }
    }
    Ok(found)
}
fn screen(v: &Value) -> Value {
    py::or(
        v.get("screen"),
        &py::string(if py::has(v, "results") {
            "mac"
        } else {
            "iphone"
        }),
    )
    .clone()
}
fn entries(v: &Value) -> Result<Vec<Value>> {
    if !py::truth(v.get("results")) {
        return Ok(vec![]);
    }
    Ok(py::iter(v.get("results"))?
        .into_iter()
        .filter(|v| matches!(v, Value::Object(_)))
        .collect())
}
fn included(v: &Value, since: &Value) -> Result<bool> {
    if py::eq(v.get("how"), "dropped") {
        return Ok(false);
    }
    Ok(py::cmp(
        py::or(v.get("finishedAt"), &Value::Number("0".into())),
        since,
    )? != Some(Ordering::Less))
}
fn unclear(kind: &Value, e: &Value) -> Result<Value> {
    if (py::truth(e.get("unclear")) && py::eq(e.get("state"), "problem"))
        || (py::eq(kind, "explain")
            && py::eq(e.get("state"), "problem")
            && !py::truth(e.get("question")))
    {
        return Ok(py::string("key"));
    }
    let empty = py::string("");
    let note = py::text(py::or(e.get("note"), &empty))?;
    let note = fold(note);
    if note.windows(2).enumerate().any(|(i, p)| {
        p == [u32::from('н'), u32::from('е')] && {
            let mut j = i + 2;
            while j < note.len() && py::space(note[j]) {
                j += 1;
            }
            note[j..].starts_with(&py::s("пон"))
        }
    }) || [
        "непонят",
        "невнятн",
        "без контекста",
        "что за ",
        "что значит",
        "а что от меня",
        "это кто",
        "нет конкретики",
        "чё тут вообще",
        "не увидел где",
    ]
    .iter()
    .any(|s| contains(&note, &py::s(s)))
    {
        Ok(py::string("words"))
    } else {
        Ok(Value::Null)
    }
}
fn contains(t: &[u32], n: &[u32]) -> bool {
    t.windows(n.len()).any(|w| w == n)
}
fn fold(t: &[u32]) -> Text {
    // Python re.I uses simple lowercase here, not full Unicode case folding.
    // In particular U+1C80 must NOT acquire the meaning of the letter в.
    t.iter()
        .map(|&n| {
            char::from_u32(n)
                .and_then(|c| {
                    let mut lower = c.to_lowercase();
                    let a = lower.next()?;
                    if lower.next().is_some() {
                        return Some(c);
                    }
                    Some(a)
                })
                .map(u32::from)
                .unwrap_or(n)
        })
        .collect()
}
fn chosen(e: &Value) -> Result<Value> {
    if py::truth(e.get("choice")) {
        return Ok(e.get("choice").clone());
    }
    let mut choices = if py::truth(e.get("choices")) {
        py::iter(e.get("choices"))?
    } else {
        vec![]
    };
    if py::truth(e.get("other")) {
        choices.push(py::string(if choices.is_empty() {
            "своё, в комментарии"
        } else {
            "и своё, в комментарии"
        }));
    }
    Ok(Value::String(py::join(
        &choices
            .iter()
            .map(|v| py::text(v).map(Vec::from))
            .collect::<Result<Vec<_>>>()?,
        "; ",
    )))
}
fn field(v: &Value, k: &str) -> Value {
    py::or(v.get(k), &py::string("")).clone()
}
fn row(v: &Value, e: &Value, screen: &Value) -> Result<Value> {
    let kind = if py::eq(screen, "mac") {
        py::or(e.get("kind"), v.get("kind")).clone()
    } else {
        Value::Null
    };
    let missing_label = py::string("?");
    let mut fields = vec![
            (
                "when",
                py::or(v.get("finishedAt"), &Value::Number("0".into())).clone(),
            ),
            ("screen", screen.clone()),
            ("chat", py::or(v.get("chat"), &field(v, "tag")).clone()),
            ("agent", field(v, "agent")),
            ("kind", kind.clone()),
            ("title", field(v, "title")),
            ("check", field(v, "id")),
            (
                "label",
                Value::String(py::repr(
                    if py::has(e, "label") {
                        e.get("label")
                    } else {
                        &missing_label
                    },
                    false,
                )?),
            ),
            ("text", field(e, "text")),
            ("state", e.get("state").clone()),
            ("choice", chosen(e)?),
            (
                "note",
                Value::String(py::strip(py::text(&field(e, "note"))?)),
            ),
            ("unclear", unclear(&kind, e)?),
        ];
    // What the item offered beside its text, kept in the history since 2026-11-03: only where it is there, so a
    // line of an older check reads as it did.
    for key in ["options", "many", "details", "facts", "links", "copies", "images", "page"] {
        if py::has(e, key) {
            fields.push((key, e.get(key).clone()));
        }
    }
    Ok(Value::Object(fields.into_iter().map(|(k, v)| (py::s(k), v)).collect()))
}
pub fn rows(root: &Path, days: &BigInt, now: f64) -> Result<Vec<Value>> {
    rows_with_limit(root, days, now, depth_limit())
}
pub fn json_rows(root: &Path, days: &BigInt, now: f64) -> Result<Vec<Value>> {
    rows_with_limit(root, days, now, depth_limit() + 1)
}
fn rows_with_limit(root: &Path, days: &BigInt, now: f64, limit: usize) -> Result<Vec<Value>> {
    let cutoff = since(days, now)?;
    let mut found = vec![];
    for v in history(root, limit)? {
        if !included(&v, &cutoff)? {
            continue;
        }
        let screen = screen(&v);
        for e in entries(&v)? {
            found.push(row(&v, &e, &screen)?);
        }
    }
    let partial = found
        .iter()
        .any(|r| py::contains_nan(r.get("when")) || py::contains_nan(r.get("check")));
    if partial {
        let order = partial_sort::sort(found.len(), |a, b| {
            Ok(row_order(&found[a], &found[b])? == Some(Ordering::Less))
        })?;
        let mut values: Vec<_> = found.into_iter().map(Some).collect();
        found = order
            .into_iter()
            .map(|i| values[i].take().unwrap())
            .collect();
    } else {
        let mut error = None;
        found.sort_by(|a, b| match row_order(a, b) {
            Ok(order) => order.unwrap_or(Ordering::Equal),
            Err(e) => {
                error = Some(e);
                Ordering::Equal
            }
        });
        if let Some(e) = error {
            return Err(e);
        }
    }
    Ok(found)
}
fn row_order(a: &Value, b: &Value) -> Result<Option<Ordering>> {
    for (key, reverse) in [("when", true), ("check", false), ("label", false)] {
        if key == "when" && (py::nan(a.get(key)) || py::nan(b.get(key))) {
            return Ok(None);
        }
        if py::equal(a.get(key), b.get(key))? {
            continue;
        }
        return Ok(py::cmp(a.get(key), b.get(key))?.map(|o| if reverse { o.reverse() } else { o }));
    }
    Ok(Some(Ordering::Equal))
}

fn kind_name(v: &Value) -> Option<&'static str> {
    for (key, name) in [
        ("checklist", "чек-лист"),
        ("result", "результат"),
        ("plan", "план"),
        ("question", "вопрос"),
        ("action", "действие"),
        ("explain", "объяснение"),
    ] {
        if py::eq(v, key) {
            return Some(name);
        }
    }
    None
}
fn date(v: &Value, full: bool) -> Result<Text> {
    let stamp = py::num(v)?.float()?;
    if !stamp.is_finite() {
        return Err("invalid timestamp");
    }
    let mut sec = stamp.floor();
    if ((stamp - sec) * 1_000_000.).round_ties_even() == 1_000_000. {
        sec += 1.;
    }
    if sec < i64::MIN as f64 || sec >= i64::MAX as f64 {
        return Err("timestamp out of range");
    }
    let stamp = sec as libc::time_t;
    let mut tm = std::mem::MaybeUninit::<libc::tm>::uninit();
    // localtime_r writes tm only when it returns a non-null pointer.
    let tm = unsafe {
        if libc::localtime_r(&stamp, tm.as_mut_ptr()).is_null() {
            return Err("timestamp out of range");
        }
        tm.assume_init()
    };
    if !(-1899..=8099).contains(&tm.tm_year) {
        return Err("year out of range");
    }
    Ok(py::s(&if full {
        format!(
            "{:02}.{:02} {:02}:{:02}",
            tm.tm_mday,
            tm.tm_mon + 1,
            tm.tm_hour,
            tm.tm_min
        )
    } else {
        format!("{:02}.{:02}", tm.tm_mday, tm.tm_mon + 1)
    }))
}
#[derive(Clone, Debug)]
struct Chat {
    checks: usize,
    items: Num,
    seconds: Num,
    unclear: usize,
}
fn by_chat(root: &Path, days: &BigInt, now: f64) -> Result<Vec<Text>> {
    let cutoff = since(days, now)?;
    let mut chats: BTreeMap<Text, Chat> = BTreeMap::new();
    for v in history(root, depth_limit())? {
        if !included(&v, &cutoff)? {
            continue;
        }
        let screen = screen(&v);
        let mac = py::eq(&screen, "mac");
        let empty = py::string("");
        let fallback = py::string("чат без имени");
        let project = py::or(v.get("projectName"), &empty);
        let chat = py::or(v.get("chat"), py::or(v.get("tag"), &fallback));
        let name = py::join(
            &[project, chat]
                .into_iter()
                .filter(|v| py::truth(v))
                .map(|v| py::text(v).map(Vec::from))
                .collect::<Result<Vec<_>>>()?,
            " · ",
        );
        let entry = chats.entry(name).or_insert(Chat {
            checks: 0,
            items: Num::Int(0.into()),
            seconds: Num::Float(0.),
            unclear: 0,
        });
        entry.checks += 1;
        let results = entries(&v)?;
        let mut count = Num::Int(results.len().into());
        if results.is_empty() && py::truth(v.get("marks")) {
            let Value::Object(marks) = v.get("marks") else {
                return Err("marks has no values");
            };
            for (_, value) in marks {
                count = count.add(&py::num(value)?)?;
            }
        }
        entry.items = entry.items.add(&count)?;
        let shown = v.get(if mac { "shownAt" } else { "installedAt" });
        if py::truth(shown) {
            let duration = py::num(py::or(v.get("finishedAt"), &Value::Number("0".into())))?
                .sub(&py::num(shown)?)?;
            entry.seconds = entry.seconds.add(&if duration.cmp(&Num::Int(0.into()))
                == Some(Ordering::Less)
            {
                Num::Int(0.into())
            } else {
                duration
            })?;
        }
        for e in results {
            let kind = if mac {
                py::or(e.get("kind"), v.get("kind")).clone()
            } else {
                Value::Null
            };
            entry.unclear += usize::from(py::truth(&unclear(&kind, &e)?));
        }
    }
    let mut chats: Vec<_> = chats.into_iter().collect();
    if chats
        .iter()
        .any(|(_, c)| matches!(&c.seconds,Num::Float(f) if f.is_nan()))
    {
        let order = partial_sort::sort(chats.len(), |a, b| {
            let (an, a) = &chats[a];
            let (bn, b) = &chats[b];
            Ok(match a.seconds.cmp(&b.seconds) {
                None => false,
                Some(Ordering::Equal) => an < bn,
                Some(o) => o == Ordering::Greater,
            })
        })?;
        chats = order.into_iter().map(|i| chats[i].clone()).collect();
    } else {
        chats.sort_by(|(an, a), (bn, b)| {
            a.seconds
                .cmp(&b.seconds)
                .unwrap()
                .reverse()
                .then(an.cmp(bn))
        });
    }
    let mut out = vec![];
    for (name, chat) in chats.iter().take(12) {
        let minutes = Num::Float((chat.seconds.float()? / 60.).round_ties_even()).integer_text()?;
        let tail = if chat.unclear > 0 {
            format!(", «не понял» {}", chat.unclear)
        } else {
            String::new()
        };
        out.push(py::cat(&[
            py::s("  "),
            name.clone(),
            py::s(&format!(
                ": проверок {}, пунктов {}, минут {minutes}{tail}",
                chat.checks,
                chat.items.integer_text()?
            )),
        ]));
    }
    if chats.len() > 12 {
        out.push(py::s(&format!("  и ещё чатов: {}", chats.len() - 12)));
    }
    Ok(out)
}
fn counted(rows: &[&Value], key: &str, kinds: bool) -> Result<Text> {
    let mut indices: HashMap<String, usize> = HashMap::new();
    let mut counts: Vec<(Value, usize)> = vec![];
    for row in rows {
        let default = py::string("");
        let name = py::or(row.get(key), &default);
        py::hash_key(name)?;
        let name = if kinds {
            if !py::truth(name) {
                py::string("на телефоне")
            } else if let Some(n) = kind_name(name) {
                py::string(n)
            } else {
                name.clone()
            }
        } else if py::truth(name) {
            name.clone()
        } else {
            py::string("не записан")
        };
        let hash = py::hash_key(&name)?;
        if let Some(&i) = indices.get(&hash) {
            counts[i].1 += 1;
        } else {
            indices.insert(hash, counts.len());
            counts.push((name, 1));
        }
    }
    let order = partial_sort::sort(counts.len(), |a, b| {
        if counts[a].1 != counts[b].1 {
            return Ok(counts[a].1 > counts[b].1);
        }
        Ok(py::cmp(&counts[a].0, &counts[b].0)? == Some(Ordering::Less))
    })?;
    Ok(py::join(
        &order
            .into_iter()
            .map(|i| {
                Ok(py::cat(&[
                    py::repr(&counts[i].0, false)?,
                    py::s(&format!(" {}", counts[i].1)),
                ]))
            })
            .collect::<Result<Vec<_>>>()?,
        ", ",
    ))
}

fn head(row: &Value) -> Result<Text> {
    // dict.get must reject unhashable kinds, even for the fallback.
    py::hash_key(py::or(row.get("kind"), &py::string("")))?;
    let kind = kind_name(row.get("kind")).unwrap_or(if py::eq(row.get("screen"), "iphone") {
        "на телефоне"
    } else {
        ""
    });
    let fallback = py::string("чат без имени");
    let mut parts = vec![
        date(row.get("when"), true)?,
        py::text(py::or(row.get("chat"), &fallback))?.to_vec(),
    ];
    if py::truth(row.get("agent")) {
        parts.push(py::text(row.get("agent"))?.to_vec());
    }
    parts.push(py::cat(&[
        py::s(kind),
        py::s(" «"),
        py::repr(row.get("title"), false)?,
        py::s("»"),
    ]));
    Ok(py::join(&parts, " · "))
}
fn address(t: &[u32]) -> bool {
    if t.starts_with(&[47]) || t.starts_with(&[126, 47]) {
        return true;
    }
    let Some(colon) = t.iter().position(|&c| c == 58) else {
        return false;
    };
    let ascii_alpha = |c: u32| matches!(c,65..=90|97..=122);
    colon > 0
        && ascii_alpha(t[0])
        && t[1..colon]
            .iter()
            .all(|&c| ascii_alpha(c) || matches!(c, 48..=57 | 43 | 45 | 46))
        && colon + 1 < t.len()
        && t[colon + 1..].iter().all(|&c| !py::space(c))
}
fn read_text(t: &[u32]) -> Text {
    let mut out = vec![];
    let mut at = 0;
    while at < t.len() {
        if t[at] != 91 {
            out.push(t[at]);
            at += 1;
            continue;
        }
        let start = at;
        let mut end = at + 1;
        while end < t.len() && !matches!(t[end], 91 | 93 | 10) {
            end += 1;
        }
        if end == at + 1 || t.get(end..end + 2) != Some(&[93, 40]) {
            out.push(t[at]);
            at += 1;
            continue;
        }
        let target = end + 2;
        let mut close = target;
        let mut inside = false;
        let mut valid = true;
        while close < t.len() {
            match t[close] {
                10 => {
                    valid = false;
                    break;
                }
                40 => {
                    if inside {
                        valid = false;
                        break;
                    }
                    inside = true;
                }
                41 => {
                    if inside {
                        inside = false;
                    } else {
                        break;
                    }
                }
                _ => {}
            }
            close += 1;
        }
        if !valid || inside || close == target || close >= t.len() {
            out.push(t[at]);
            at += 1;
            continue;
        }
        let label = py::strip(&t[at + 1..end]);
        if !label.is_empty() && address(&py::strip(&t[target..close])) {
            out.extend(label);
        } else {
            out.extend(&t[start..close + 1]);
        }
        at = close + 1;
    }
    out
}
fn lines(row: &Value) -> Result<Vec<Text>> {
    let asked = read_text(py::text(row.get("text"))?);
    let asked = if asked.is_empty() {
        py::s("(текст пункта не сохранён: проверка старше 11.10.2026)")
    } else {
        asked
    };
    let mut out = vec![py::cat(&[
        py::s("  "),
        py::text(row.get("label"))?.to_vec(),
        py::s(". "),
        asked,
    ])];
    if py::truth(row.get("choice")) {
        out.push(py::cat(&[
            py::s("  выбрал: "),
            py::repr(row.get("choice"), false)?,
        ]));
    }
    out.push(if py::truth(row.get("note")) {
        py::cat(&[py::s("  » "), py::collapse(py::text(row.get("note"))?)])
    } else {
        py::s("  » без комментария")
    });
    Ok(out)
}
/// The words he asked about in his comment, «что за X», «что такое X»: what a chat said to him in a word of its
/// own (his choice of 2026-11-03, «Отчёт сам копит слова из твоих «что за X»»). Counted by the phrase after the
/// question, up to three words; a word goes into the refusals of `hitl check` only on his «да».
fn asked_words(rows: &[&Value]) -> Result<Vec<(String, usize)>> {
    const MARKERS: [&str; 6] = ["что за ", "чё за ", "че за ", "что такое ", "что значит ", "какой конкретно "];
    const SKIP: [&str; 13] = ["это", "он", "она", "оно", "они", "тут", "там", "бред", "хуйня", "чушь", "каша", "ерунда", "вопрос был"];
    const TAIL: [&str; 14] = ["в", "и", "я", "не", "на", "с", "а", "но", "то", "это", "что", "как", "же", "вообще"];
    let mut counts: Vec<(String, usize)> = Vec::new();
    for row in rows {
        let Value::String(note) = row.get("note") else { continue };
        let note: String = note.iter().map(|&c| char::from_u32(c).unwrap_or(' ')).collect::<String>().to_lowercase();
        for marker in MARKERS {
            let Some(at) = note.find(marker) else { continue };
            let rest = &note[at + marker.len()..];
            let rest = rest.split(|c: char| "?.,!;:\n".contains(c)).next().unwrap_or("");
            let mut words: Vec<&str> = rest.split_whitespace().take(3).collect();
            while words.last().is_some_and(|word| TAIL.contains(&word.trim_matches(|c: char| "«»\"“”„".contains(c)))) {
                words.pop();
            }
            let phrase = words.join(" ");
            let phrase = phrase.trim_matches(|c: char| "«»\"“”„ ".contains(c)).to_string();
            if phrase.chars().count() < 3 || SKIP.contains(&phrase.as_str()) {
                continue;
            }
            match counts.iter_mut().find(|(known, _)| *known == phrase) {
                Some((_, number)) => *number += 1,
                None => counts.push((phrase, 1)),
            }
            break;
        }
    }
    counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    Ok(counts)
}
pub fn report(root: &Path, days: &BigInt, comments: bool, now: impl Fn() -> f64) -> Result<Text> {
    let found = rows(root, days, now())?;
    let period = if days == &BigInt::from(0) {
        "за всё время".into()
    } else {
        format!("за {days} дн.")
    };
    if found.is_empty() {
        return Ok(py::s(&format!(
            "Проверок с ответами {period} в истории нет."
        )));
    }
    let mut checks = HashSet::new();
    for row in &found {
        checks.insert((
            py::hash_key(row.get("check"))?,
            py::hash_key(row.get("screen"))?,
        ));
    }
    let mac = checks
        .iter()
        .filter(|(_, s)| s == &py::hash_key(&py::string("mac")).unwrap())
        .count();
    let phone = checks
        .iter()
        .filter(|(_, s)| s == &py::hash_key(&py::string("iphone")).unwrap())
        .count();
    let noted: Vec<_> = found.iter().filter(|r| py::truth(r.get("note"))).collect();
    let unclear: Vec<_> = found
        .iter()
        .filter(|r| py::truth(r.get("unclear")))
        .collect();
    let by_key = unclear
        .iter()
        .filter(|r| py::eq(r.get("unclear"), "key"))
        .count();
    let mut out = vec![
        py::cat(&[
            py::s(&format!("Проверки {period}, с ")),
            date(found.last().unwrap().get("when"), false)?,
            py::s(" по "),
            date(found[0].get("when"), false)?,
            py::s(&format!(
                ": {mac} в окне на Маке, {phone} на телефоне; пунктов {}.",
                found.len()
            )),
        ]),
        py::s(&format!(
            "Его комментариев: {}. «Не понял»: {} — ответом {by_key}, словами в комментарии {}.",
            noted.len(),
            unclear.len(),
            unclear.len() - by_key
        )),
        vec![],
        py::s("Сколько занял каждый чат, от самого долгого:"),
    ];
    out.extend(by_chat(root, days, now())?);
    if !unclear.is_empty() {
        out.extend([
            vec![],
            py::cat(&[
                py::s("По виду проверки: "),
                counted(&unclear, "kind", true)?,
                py::s("."),
            ]),
            py::cat(&[
                py::s("По агенту: "),
                counted(&unclear, "agent", false)?,
                py::s("."),
            ]),
            py::cat(&[
                py::s("По чату: "),
                counted(&unclear, "chat", false)?,
                py::s("."),
            ]),
            vec![],
        ]);
        let asked = asked_words(&unclear)?;
        if !asked.is_empty() {
            let shown: Vec<String> = asked.iter().take(15).map(|(word, number)| format!("«{word}» {number}")).collect();
            out.push(py::s(&format!("Слова, о которых он переспросил (в отказы они идут на его «да»): {}.", shown.join(", "))));
            out.push(vec![]);
        }
        out.push(py::s("«Не понял», от новых к старым:"));
        for row in unclear {
            out.push(head(row)?);
            out.extend(lines(row)?);
        }
    }
    if comments {
        let rest: Vec<_> = noted
            .into_iter()
            .filter(|r| !py::truth(r.get("unclear")))
            .collect();
        out.extend([
            vec![],
            py::s(&format!("Остальные комментарии, {}:", rest.len())),
        ]);
        for row in rest {
            out.push(head(row)?);
            out.extend(lines(row)?);
        }
    }
    Ok(py::join(&out, "\n"))
}
pub fn output_line(text: &[u32], errors: &str) -> Result<Vec<u8>> {
    py::encode(&py::cat(&[text.to_vec(), py::s("\n")]), errors)
}
pub fn json_line(row: &Value, errors: &str) -> Result<Vec<u8>> {
    output_line(&py::json(row)?, errors)
}
pub fn selected(row: &Value, comments: bool) -> bool {
    py::truth(row.get("unclear")) || (comments && py::truth(row.get("note")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::Cell,
        fs,
        sync::atomic::{AtomicUsize, Ordering as AtomicOrdering},
    };
    static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "hitl-stats-unit-{}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, AtomicOrdering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn write(&self, history: &str) {
            fs::write(self.0.join("history.jsonl"), history).unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
    #[test]
    fn cutoff_before_at_after_and_zero_days() {
        let f = Fixture::new();
        f.write("{\"id\":\"before\",\"finishedAt\":13599.99,\"results\":[{}]}\n{\"id\":\"at\",\"finishedAt\":13600,\"results\":[{}]}\n{\"id\":\"after\",\"finishedAt\":13600.01,\"results\":[{}]}\n");
        let r = rows(&f.0, &1.into(), 100000.).unwrap();
        assert_eq!(r.len(), 2);
        assert!(py::eq(r[0].get("check"), "after"));
        assert!(py::eq(r[1].get("check"), "at"));
        assert_eq!(rows(&f.0, &0.into(), 100000.).unwrap().len(), 3);
    }
    #[test]
    fn text_report_keeps_two_unlocked_reads_with_independent_cutoffs() {
        let f = Fixture::new();
        let first = "{\"id\":\"first\",\"finishedAt\":100000,\"shownAt\":99940,\"chat\":\"one\",\"results\":[{}]}\n";
        f.write(first);
        let calls = Cell::new(0);
        let text=report(&f.0,&1.into(),false,||{
            let n=calls.get();calls.set(n+1);
            if n==1{f.write(&format!("{first}{{\"id\":\"late\",\"finishedAt\":100001,\"chat\":\"two\",\"results\":[{{}}]}}\n"));}
            if n==0 {100100.}else{186401.}
        }).unwrap();
        let text = py::unicode(&text);
        assert_eq!(calls.get(), 2);
        assert!(text.contains("пунктов 1."));
        assert!(text.contains("two: проверок 1"));
        assert!(!text.contains("one: проверок 1"));
        assert!(
            fs::read_to_string(f.0.join("history.jsonl"))
                .unwrap()
                .contains("late")
        );
    }
}
