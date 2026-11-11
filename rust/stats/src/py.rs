//! The small Python value operations used by the historical report. Unexpected
//! types fail the report, just as the Python reader does; they are not repaired.
use hitl_compat_json::Value;
use num_bigint::BigInt;
use num_traits::{FromPrimitive, ToPrimitive, Zero};
use std::cmp::Ordering;

pub type Result<T> = std::result::Result<T, &'static str>;
pub type Text = Vec<u32>;
pub fn s(text: &str) -> Text {
    text.chars().map(u32::from).collect()
}
pub fn string(text: &str) -> Value {
    Value::String(s(text))
}
pub fn eq(v: &Value, text: &str) -> bool {
    matches!(v, Value::String(t) if t.iter().copied().eq(text.chars().map(u32::from)))
}
pub fn nan(v: &Value) -> bool {
    matches!(v,Value::Number(n) if n=="NaN")
}
pub fn contains_nan(v: &Value) -> bool {
    match v {
        Value::Array(a) => a.iter().any(contains_nan),
        Value::Object(o) => o.iter().any(|(_, v)| contains_nan(v)),
        _ => nan(v),
    }
}
pub fn has(v: &Value, key: &str) -> bool {
    matches!(v, Value::Object(o) if o.iter().any(|(k,_)| k == &s(key)))
}
pub fn truth(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(v) => *v,
        Value::String(v) => !v.is_empty(),
        Value::Array(v) => !v.is_empty(),
        Value::Object(v) => !v.is_empty(),
        Value::Number(_) => match num(v) {
            Ok(Num::Int(v)) => !v.is_zero(),
            Ok(Num::Float(v)) => v != 0.,
            Err(_) => true,
        },
    }
}
pub fn or<'a>(a: &'a Value, b: &'a Value) -> &'a Value {
    if truth(a) { a } else { b }
}
pub fn text(v: &Value) -> Result<&[u32]> {
    match v {
        Value::String(t) => Ok(t),
        _ => Err("expected a string"),
    }
}
#[cfg(test)]
pub fn unicode(t: &[u32]) -> String {
    // Only for matching. Surrogates are not whitespace or letters in a phrase.
    t.iter()
        .map(|&c| char::from_u32(c).unwrap_or('\u{fffd}'))
        .collect()
}
pub fn space(c: u32) -> bool {
    matches!(c, 9..=13 | 28..=32 | 0x85 | 0xa0 | 0x1680 | 0x2000..=0x200a | 0x2028 | 0x2029 | 0x202f | 0x205f | 0x3000)
}
pub fn strip(t: &[u32]) -> Text {
    let start = t.iter().position(|&c| !space(c)).unwrap_or(t.len());
    let end = t.iter().rposition(|&c| !space(c)).map_or(start, |i| i + 1);
    t[start..end].to_vec()
}
pub fn collapse(t: &[u32]) -> Text {
    let mut out = vec![];
    let mut pending = false;
    for &c in t {
        if space(c) {
            pending = !out.is_empty();
        } else {
            if pending {
                out.push(32);
            }
            out.push(c);
            pending = false;
        }
    }
    out
}
pub fn join(parts: &[Text], separator: &str) -> Text {
    let sep = s(separator);
    let mut out = vec![];
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            out.extend(&sep);
        }
        out.extend(p);
    }
    out
}
pub fn cat(parts: &[Text]) -> Text {
    join(parts, "")
}
pub fn iter(v: &Value) -> Result<Vec<Value>> {
    match v {
        Value::Array(a) => Ok(a.clone()),
        Value::Object(o) => Ok(o.iter().map(|(k, _)| Value::String(k.clone())).collect()),
        Value::String(s) => Ok(s.iter().map(|&c| Value::String(vec![c])).collect()),
        _ => Err("object is not iterable"),
    }
}
#[derive(Clone, Debug)]
pub enum Num {
    Int(BigInt),
    Float(f64),
}
pub fn num(v: &Value) -> Result<Num> {
    match v {
        Value::Bool(b) => Ok(Num::Int(BigInt::from(u8::from(*b)))),
        Value::Number(n)
            if !n.contains(['.', 'e', 'E'])
                && !matches!(n.as_str(), "NaN" | "Infinity" | "-Infinity") =>
        {
            n.parse::<BigInt>()
                .map(Num::Int)
                .map_err(|_| "invalid integer")
        }
        Value::Number(n) => n
            .parse::<f64>()
            .map(Num::Float)
            .map_err(|_| "invalid float"),
        _ => Err("expected a number"),
    }
}
impl Num {
    pub fn float(&self) -> Result<f64> {
        match self {
            Self::Int(n) => n
                .to_f64()
                .filter(|f| f.is_finite())
                .ok_or("integer too large to convert to float"),
            Self::Float(n) => Ok(*n),
        }
    }
    pub fn add(&self, b: &Self) -> Result<Self> {
        Ok(match (self, b) {
            (Self::Int(a), Self::Int(b)) => Self::Int(a + b),
            _ => Self::Float(self.float()? + b.float()?),
        })
    }
    pub fn sub(&self, b: &Self) -> Result<Self> {
        Ok(match (self, b) {
            (Self::Int(a), Self::Int(b)) => Self::Int(a - b),
            _ => Self::Float(self.float()? - b.float()?),
        })
    }
    pub fn cmp(&self, b: &Self) -> Option<Ordering> {
        match (self, b) {
            (Self::Int(a), Self::Int(b)) => Some(a.cmp(b)),
            (Self::Float(a), Self::Float(b)) => a.partial_cmp(b),
            (Self::Int(a), Self::Float(b)) => {
                if b.is_nan() {
                    return None;
                }
                if b.is_infinite() {
                    return Some(if b.is_sign_positive() {
                        Ordering::Less
                    } else {
                        Ordering::Greater
                    });
                }
                let whole = BigInt::from_f64(*b).unwrap();
                Some(
                    a.cmp(&whole)
                        .then_with(|| 0f64.partial_cmp(&b.fract()).unwrap()),
                )
            }
            (Self::Float(_), Self::Int(_)) => b.cmp(self).map(Ordering::reverse),
        }
    }
    pub fn integer_text(&self) -> Result<String> {
        match self {
            Self::Int(n) => Ok(n.to_string()),
            Self::Float(f) => BigInt::from_f64(*f)
                .map(|n| n.to_string())
                .ok_or("cannot convert nonfinite to integer"),
        }
    }
}
pub fn cmp(a: &Value, b: &Value) -> Result<Option<Ordering>> {
    match (a, b) {
        (Value::String(a), Value::String(b)) => Ok(Some(a.cmp(b))),
        (Value::Array(a), Value::Array(b)) => {
            for (a, b) in a.iter().zip(b) {
                if equal(a, b)? {
                    continue;
                }
                return cmp(a, b);
            }
            Ok(Some(a.len().cmp(&b.len())))
        }
        _ => Ok(num(a)?.cmp(&num(b)?)),
    }
}
pub fn equal(a: &Value, b: &Value) -> Result<bool> {
    // json.loads uses one shared NaN constant. Python tuple/list equality
    // checks object identity before rich comparison. A negated sort timestamp,
    // by contrast, is a fresh float; row_order handles that separately.
    if nan(a) && nan(b) {
        return Ok(true);
    }
    Ok(match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::String(a), Value::String(b)) => a == b,
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| equal(a, b).unwrap_or(false))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter().all(|(k, v)| {
                    b.iter()
                        .any(|(l, w)| k == l && equal(v, w).unwrap_or(false))
                })
        }
        _ => num(a)
            .ok()
            .zip(num(b).ok())
            .is_some_and(|(a, b)| a.cmp(&b) == Some(Ordering::Equal)),
    })
}
pub fn hash_key(v: &Value) -> Result<String> {
    Ok(match v {
        Value::Null => "none".into(),
        Value::String(t) => format!("s{t:?}"),
        _ => match num(v)? {
            Num::Int(n) => format!("n{n}"),
            Num::Float(f) if f.fract() == 0. && f.is_finite() => {
                format!("n{}", BigInt::from_f64(f).unwrap())
            }
            Num::Float(f) => format!("f{f:?}"),
        },
    })
}
pub fn repr(v: &Value, nested: bool) -> Result<Text> {
    Ok(match v {
        Value::Null => s("None"),
        Value::Bool(b) => s(if *b { "True" } else { "False" }),
        Value::String(t) if !nested => t.clone(),
        Value::String(t) => s(&string_repr(t)),
        Value::Number(_) => s(&match num(v)? {
            Num::Int(n) => n.to_string(),
            Num::Float(f) => float_repr(f),
        }),
        Value::Array(a) => cat(&[
            s("["),
            join(
                &a.iter()
                    .map(|v| repr(v, true))
                    .collect::<Result<Vec<_>>>()?,
                ", ",
            ),
            s("]"),
        ]),
        Value::Object(o) => cat(&[
            s("{"),
            join(
                &o.iter()
                    .map(|(k, v)| Ok(cat(&[s(&string_repr(k)), s(": "), repr(v, true)?])))
                    .collect::<Result<Vec<_>>>()?,
                ", ",
            ),
            s("}"),
        ]),
    })
}
// The exact shortest-roundtrip and exponent policy already used by journal's
// Python compatibility codec; stats also needs it for JSONL and label str().
pub fn float_repr(f: f64) -> String {
    if f.is_nan() {
        return "nan".into();
    }
    if !f.is_finite() {
        return if f.is_sign_negative() { "-inf" } else { "inf" }.into();
    }
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
fn string_repr(t: &[u32]) -> String {
    let quote = if t.contains(&39) && !t.contains(&34) {
        34
    } else {
        39
    };
    let mut out = String::from(char::from_u32(quote).unwrap());
    for &c in t {
        match c {
            9 => out.push_str("\\t"),
            10 => out.push_str("\\n"),
            13 => out.push_str("\\r"),
            92 => out.push_str("\\\\"),
            c if c == quote => {
                out.push('\\');
                out.push(char::from_u32(c).unwrap());
            }
            c if !printable(c) => out.push_str(&if c <= 255 {
                format!("\\x{c:02x}")
            } else if c <= 65535 {
                format!("\\u{c:04x}")
            } else {
                format!("\\U{c:08x}")
            }),
            c => out.push(char::from_u32(c).unwrap()),
        }
    }
    out.push(char::from_u32(quote).unwrap());
    out
}
fn printable(v: u32) -> bool {
    let n = NONPRINTING.partition_point(|&(_, end)| end < v);
    NONPRINTING
        .get(n)
        .is_none_or(|&(start, end)| v < start || v > end)
}
// Read-only data shared with the first consumer, without changing its codec.
include!("../../journal/src/nonprinting.rs");

pub fn json(v: &Value) -> Result<Text> {
    Ok(match v {
        Value::Null => s("null"),
        Value::Bool(b) => s(if *b { "true" } else { "false" }),
        Value::Number(_) => s(&match num(v)? {
            Num::Int(n) => n.to_string(),
            Num::Float(f) if f.is_nan() => "NaN".into(),
            Num::Float(f) if f == f64::INFINITY => "Infinity".into(),
            Num::Float(f) if f == f64::NEG_INFINITY => "-Infinity".into(),
            Num::Float(f) => float_repr(f),
        }),
        Value::String(t) => {
            let mut out = s("\"");
            for &c in t {
                out.extend(match c {
                    34 => s("\\\""),
                    92 => s("\\\\"),
                    8 => s("\\b"),
                    12 => s("\\f"),
                    10 => s("\\n"),
                    13 => s("\\r"),
                    9 => s("\\t"),
                    0..=31 => s(&format!("\\u{c:04x}")),
                    _ => vec![c],
                });
            }
            out.push(34);
            out
        }
        Value::Array(a) => cat(&[
            s("["),
            join(&a.iter().map(json).collect::<Result<Vec<_>>>()?, ", "),
            s("]"),
        ]),
        Value::Object(o) => cat(&[
            s("{"),
            join(
                &o.iter()
                    .map(|(k, v)| Ok(cat(&[json(&Value::String(k.clone()))?, s(": "), json(v)?])))
                    .collect::<Result<Vec<_>>>()?,
                ", ",
            ),
            s("}"),
        ]),
    })
}
pub fn encode(t: &[u32], errors: &str) -> Result<Vec<u8>> {
    let mut out = vec![];
    for &c in t {
        if let Some(c) = char::from_u32(c) {
            let mut buf = [0; 4];
            out.extend(c.encode_utf8(&mut buf).as_bytes());
        } else if errors == "surrogateescape" && (0xdc80..=0xdcff).contains(&c) {
            out.push((c - 0xdc00) as u8);
        } else if errors == "surrogatepass" {
            out.extend([
                (0xe0 | (c >> 12)) as u8,
                (0x80 | ((c >> 6) & 63)) as u8,
                (0x80 | (c & 63)) as u8,
            ]);
        } else if errors == "backslashreplace" || errors == "namereplace" {
            out.extend(format!("\\u{c:04x}").bytes());
        } else if errors == "xmlcharrefreplace" {
            out.extend(format!("&#{c};").bytes());
        } else if errors == "replace" {
            out.push(b'?');
        } else if errors != "ignore" {
            return Err("stdout cannot encode a surrogate");
        }
    }
    Ok(out)
}
