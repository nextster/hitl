use crate::json::Value;
use rusqlite::types::Value as Sql;
use sha2::{Digest, Sha256};

pub fn code(v: &Value, limit: usize) -> String {
    let Some(s) = v.text() else {
        return String::new();
    };
    if s.len() <= limit
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_.:/@-".contains(&c))
    {
        s
    } else {
        String::new()
    }
}
pub fn reference(v: &Value) -> Result<String, ()> {
    if matches!(v, Value::Null) || v.text().as_deref() == Some("") {
        return Ok(String::new());
    }
    let s = pystr(v, false)?;
    let c = code(&Value::String(s.chars().map(u32::from).collect()), 128);
    Ok(if !s.starts_with(['/', '~']) && !c.is_empty() {
        c
    } else {
        format!(
            "sha256:{}",
            &format!("{:x}", Sha256::digest(s.as_bytes()))[..24]
        )
    })
}
fn pystr(v: &Value, repr: bool) -> Result<String, ()> {
    Ok(match v {
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::Number(n) => {
            if !n.contains(['.', 'e', 'E']) && !n.contains("Infinity") && n != "NaN" {
                if n == "-0" { "0".into() } else { n.clone() }
            } else {
                let f = n.parse::<f64>().map_err(|_| ())?;
                float_repr(f)
            }
        }
        Value::String(s) if !repr => s
            .iter()
            .map(|&c| char::from_u32(c))
            .collect::<Option<String>>()
            .ok_or(())?,
        Value::String(s) => string_repr(s),
        Value::Array(a) => format!(
            "[{}]",
            a.iter()
                .map(|v| pystr(v, true))
                .collect::<Result<Vec<_>, _>>()?
                .join(", ")
        ),
        Value::Object(o) => format!(
            "{{{}}}",
            o.iter()
                .map(|(k, v)| Ok(format!("{}: {}", string_repr(k), pystr(v, true)?)))
                .collect::<Result<Vec<_>, ()>>()?
                .join(", ")
        ),
    })
}
fn float_repr(f: f64) -> String {
    if f.is_nan() {
        return "nan".into();
    }
    if !f.is_finite() {
        return if f.is_sign_negative() { "-inf" } else { "inf" }.into();
    }
    // Both runtimes use shortest roundtrip digits. Python switches to exponent
    // notation outside [-4,16), always with a signed two-digit exponent.
    let text = serde_json::to_string(&f).unwrap();
    let sign = if f.is_sign_negative() { "-" } else { "" };
    if f == 0. {
        return format!("{sign}0.0");
    }
    let unsigned = text.trim_start_matches('-');
    let (base, suffix) = unsigned.split_once('e').unwrap_or((unsigned, "0"));
    let dot = base.find('.').unwrap_or(base.len()) as i32;
    let digits: String = base.chars().filter(|&c| c != '.').collect();
    let leading = digits.bytes().position(|c| c != b'0').unwrap();
    let exponent = suffix.parse::<i32>().unwrap() + dot - leading as i32 - 1;
    let digits = digits[leading..].trim_end_matches('0');
    if !(-4..16).contains(&exponent) {
        let tail = if digits.len() > 1 {
            format!(".{}", &digits[1..])
        } else {
            String::new()
        };
        return format!(
            "{sign}{}{tail}e{}{:02}",
            &digits[..1],
            if exponent < 0 { "-" } else { "+" },
            exponent.abs()
        );
    }
    let position = exponent + 1;
    if position <= 0 {
        format!("{sign}0.{}{digits}", "0".repeat((-position) as usize))
    } else if position as usize >= digits.len() {
        format!(
            "{sign}{digits}{}.0",
            "0".repeat(position as usize - digits.len())
        )
    } else {
        format!(
            "{sign}{}.{}",
            &digits[..position as usize],
            &digits[position as usize..]
        )
    }
}
fn string_repr(s: &[u32]) -> String {
    let quote = if s.contains(&39) && !s.contains(&34) {
        '"'
    } else {
        '\''
    };
    let mut out = String::from(quote);
    for &v in s {
        match v {
            9 => out.push_str("\\t"),
            10 => out.push_str("\\n"),
            13 => out.push_str("\\r"),
            92 => out.push_str("\\\\"),
            v if v == u32::from(quote) => {
                out.push('\\');
                out.push(quote);
            }
            v if !printable(v) => {
                if v <= 255 {
                    out.push_str(&format!("\\x{v:02x}"));
                } else if v <= 65535 {
                    out.push_str(&format!("\\u{v:04x}"));
                } else {
                    out.push_str(&format!("\\U{v:08x}"));
                }
            }
            v => out.push(char::from_u32(v).unwrap()),
        }
    }
    out.push(quote);
    out
}
fn printable(v: u32) -> bool {
    // Generated from Python's Unicode printability, only used for legacy str(dict/list) hashing.
    let n = crate::codec::NONPRINTING.partition_point(|&(_, end)| end < v);
    NONPRINTING
        .get(n)
        .is_none_or(|&(start, end)| v < start || v > end)
}
include!("nonprinting.rs");

pub fn number(v: &Value, signed: bool) -> Sql {
    let Value::Number(n) = v else {
        return Sql::Null;
    };
    if !n.contains(['.', 'e', 'E']) && !n.contains("Infinity") && n != "NaN" {
        return n
            .parse::<i64>()
            .ok()
            .filter(|x| signed || *x >= 0)
            .map_or(Sql::Null, Sql::Integer);
    }
    n.parse::<f64>()
        .ok()
        .filter(|x| {
            x.is_finite()
                && *x >= if signed { -9223372036854775808. } else { 0. }
                && *x < 9223372036854775808.
        })
        .map_or(Sql::Null, Sql::Real)
}
fn numeric_overflow(v: &Value) -> Result<(), ()> {
    // Python math.isfinite first converts an arbitrary-sized int to float.
    // Overflow drops the whole diagnostic event through emit's exception guard.
    if let Value::Number(n) = v {
        if !n.contains(['.', 'e', 'E'])
            && !matches!(n.as_str(), "NaN" | "Infinity" | "-Infinity")
            && n.parse::<f64>().is_ok_and(|f| !f.is_finite())
        {
            return Err(());
        }
    }
    Ok(())
}
fn context_value(v: &Value) -> Result<serde_json::Value, ()> {
    numeric_overflow(v)?;
    Ok(match number(v, true) {
        Sql::Integer(n) => n.into(),
        Sql::Real(n) => serde_json::json!(n),
        _ if matches!(v, Value::Number(_)) => serde_json::Value::Null,
        _ => code(v, 80).into(),
    })
}
pub fn event(v: &Value, now: f64) -> Result<Option<Vec<Sql>>, ()> {
    let kind = v.get("kind").text().unwrap_or_default();
    let native = matches!(
        kind.as_str(),
        "native.process.launch" | "native.process.result"
    );
    let life = matches!(
        kind.as_str(),
        "hitl.panel.start"
            | "hitl.panel.start_refused"
            | "hitl.panel.exit"
            | "hitl.panel.exit_requested"
            | "hitl.card.observed"
            | "hitl.card.removed"
            | "hitl.card.left"
            | "hitl.card.shown"
            | "hitl.window.hide"
            | "hitl.window.show"
            | "hitl.window.occlusion"
            | "hitl.window.closed"
    );
    if !native
        && !life
        && !matches!(
            kind.as_str(),
            "hitl.mark" | "hitl.finish" | "hitl.postpone" | "hitl.link" | "journal.loss"
        )
    {
        return Ok(None);
    }
    let mut context = serde_json::Map::new();
    if native {
        for k in [
            "purpose",
            "executable",
            "caller",
            "launcher_pid",
            "launcher_ppid",
            "child_pid",
            "exit_code",
            "native_code",
        ] {
            if let Value::Object(fields) = v {
                if fields
                    .iter()
                    .any(|(s, _)| s.iter().copied().eq(k.chars().map(u32::from)))
                {
                    context.insert(
                        k.into(),
                        if matches!(k, "executable" | "caller") {
                            reference(v.get(k))?.into()
                        } else {
                            context_value(v.get(k))?
                        },
                    );
                }
            }
        }
    } else {
        context.insert("item".into(), context_value(v.get("item"))?);
        context.insert(
            "upstream_queue_full".into(),
            context_value(v.get("dropped"))?,
        );
        for k in [
            "producer_pid",
            "window",
            "native_window",
            "reason",
            "binary",
            "marked",
            "waiting",
        ] {
            if let Value::Object(fields) = v {
                if fields
                    .iter()
                    .any(|(s, _)| s.iter().copied().eq(k.chars().map(u32::from)))
                {
                    context.insert(k.into(), context_value(v.get(k))?);
                }
            }
        }
    }
    numeric_overflow(v.get("at"))?;
    let at = match number(v.get("at"), false) {
        Sql::Real(n) if n != 0. => n,
        Sql::Integer(n) if n != 0 => n as f64,
        _ => now,
    };
    let source = code(v.get("source"), 80);
    let phase = if native {
        let s = code(v.get("phase"), 80);
        if s.is_empty() { "event".into() } else { s }
    } else {
        "event".into()
    };
    let mut row = vec![
        Sql::Real(at),
        Sql::Null,
        Sql::Null,
        Sql::Null,
        Sql::Text(if native || life { "automation" } else { "user" }.into()),
        Sql::Text(if source.is_empty() {
            if native {
                "native.Conveyor"
            } else {
                "hitl.mac"
            }
            .into()
        } else {
            source
        }),
        Sql::Text(kind),
        Sql::Text(phase),
    ];
    for k in ["trace", "action", "parent", "request", "attempt", "subject"] {
        row.push(Sql::Text(reference(if !native && k == "trace" {
            v.get("window")
        } else if !native && matches!(k, "action" | "parent" | "attempt")
            || native && k == "attempt"
        {
            &Value::Null
        } else {
            v.get(k)
        })?));
    }
    row.extend([
        Sql::Text(code(v.get("outcome"), 80)),
        Sql::Text(String::new()),
        Sql::Null,
        Sql::Null,
        Sql::Null,
        Sql::Null,
        Sql::Null,
        Sql::Text(serde_json::to_string(&context).map_err(|_| ())?),
    ]);
    Ok(Some(row))
}
