//! What the phone's Python took from the language and the phone still reads the same way: a path as `pathlib` writes
//! it, `x or default`, a value as `str()` writes it.

use serde_json::Value;

/// A path as `str(Path(text))` writes it: no empty or `.` parts, no slash at the end, `//` kept at the start.
pub fn path(text: &str) -> String {
    if text.is_empty() {
        return ".".into();
    }
    let root = if text.starts_with("//") && !text.starts_with("///") { "//" } else if text.starts_with('/') { "/" } else { "" };
    let parts: Vec<&str> = text.split('/').filter(|part| !part.is_empty() && *part != ".").collect();
    let joined = parts.join("/");
    match (root, joined.is_empty()) {
        ("", true) => ".".into(),
        (root, _) => format!("{root}{joined}"),
    }
}

/// `str(Path(text).expanduser())`: `~` and `~/…` as the home folder.
pub fn expanded(text: &str) -> String {
    let normal = path(text);
    if normal == "~" || normal.starts_with("~/") {
        if let Some(home) = std::env::var("HOME").ok().filter(|home| !home.is_empty()) {
            return path(&format!("{home}{}", &normal[1..]));
        }
    }
    normal
}

/// `Path(text).name`: the last part.
pub fn name(text: &str) -> String {
    let normal = path(text);
    if normal == "." || normal == "/" || normal == "//" {
        return String::new();
    }
    normal.rsplit('/').next().unwrap_or("").to_string()
}

/// `Path(text).suffix`, as written: the last dot and what follows it, unless the name starts with that dot.
pub fn suffix(text: &str) -> String {
    let name = name(text);
    match name.rfind('.') {
        Some(at) if at > 0 && at < name.len() - 1 => name[at..].to_string(),
        _ => String::new(),
    }
}

/// `value or default`, where an empty string, list or object, zero, false and null are all nothing.
pub fn or(value: Option<&Value>, default: Value) -> Value {
    match value {
        Some(value) if hitl::common::truthy(Some(value)) => value.clone(),
        _ => default,
    }
}

/// `dictionary.get(key, default)`: the value when the key is there, whatever it is.
pub fn get(value: &Value, key: &str, default: Value) -> Value {
    value.get(key).cloned().unwrap_or(default)
}

/// `str(value)` of a value read from JSON.
pub fn str_of(value: &Value) -> String {
    hitl::verdict::py_str(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_as_pathlib_writes_them() {
        for (given, written) in [("/a//b/./c/", "/a/b/c"), ("a/b", "a/b"), ("", "."), ("//x", "//x"), ("///x", "/x"), ("./", "."),
                                 ("/Users/a/Мой план.md", "/Users/a/Мой план.md"), ("a/../b", "a/../b")] {
            assert_eq!(path(given), written, "{given}");
        }
        assert_eq!((name("/a/plan.md"), name("/"), name("a/b/")), ("plan.md".into(), String::new(), "b".into()));
        assert_eq!((suffix("/a/Screen.PNG"), suffix(".bashrc"), suffix("a.tar.gz"), suffix("a.")),
                   (".PNG".into(), String::new(), ".gz".into(), String::new()));
        assert!(!expanded("~/x").starts_with('~'));
        assert_eq!(expanded("~other/x"), "~other/x");
    }
}
