//! The existing reader uses Python's JSON dialect (including nonfinite numbers
//! and lone surrogate escapes). Keep those values until the field sanitizer;
//! accepting a JSON object is not permission to persist its arbitrary fields.
#[derive(Clone, Debug)]
pub enum Value {
    Null,
    Bool(bool),
    Number(String),
    String(Vec<u32>),
    Array(Vec<Value>),
    Object(Vec<(Vec<u32>, Value)>),
}
impl Value {
    pub fn get(&self, key: &str) -> &Value {
        if let Value::Object(fields) = self {
            if let Some((_, value)) = fields
                .iter()
                .find(|(k, _)| k.iter().copied().eq(key.chars().map(u32::from)))
            {
                return value;
            }
        }
        &Value::Null
    }
    pub fn text(&self) -> Option<String> {
        if let Value::String(s) = self {
            s.iter().map(|&c| char::from_u32(c)).collect()
        } else {
            None
        }
    }
}
/// stdin uses Python's UTF-8 surrogateescape on this platform. Encode undecodable
/// bytes as surrogate escapes inside JSON strings so field sanitizers, rather
/// than an unrelated UTF-8 rejection, determine whether the event survives.
pub fn stdin_line(bytes: &[u8]) -> (String, usize) {
    let mut out = String::new();
    let mut count = 0;
    let mut pos = 0;
    let mut quoted = false;
    let mut escaped = false;
    while pos < bytes.len() {
        let (valid, invalid) = match std::str::from_utf8(&bytes[pos..]) {
            Ok(s) => (s.len(), 0),
            Err(e) => (
                e.valid_up_to(),
                e.error_len().unwrap_or(bytes.len() - pos - e.valid_up_to()),
            ),
        };
        let text = std::str::from_utf8(&bytes[pos..pos + valid]).unwrap();
        for c in text.chars() {
            count += 1;
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' && quoted {
                escaped = true;
            } else if c == '"' {
                quoted = !quoted;
            }
        }
        pos += valid;
        for &b in &bytes[pos..pos + invalid] {
            count += 1;
            if quoted && !escaped {
                out.push_str(&format!("\\udc{b:02x}"));
            } else {
                out.push('?');
                escaped = false;
            }
        }
        pos += invalid;
    }
    (out, count)
}
pub fn parse(line: &str) -> Result<Value, ()> {
    parse_with_limit(line, 1000).map_err(|_| ())
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseError {
    Invalid,
    Depth,
}
/// Distinguish the stats reader's fatal RecursionError from invalid JSON lines.
/// The journal entry point retains its original 1000-depth/Err(()) contract.
pub fn parse_with_limit(line: &str, limit: usize) -> Result<Value, ParseError> {
    let mut parser = Parser {
        chars: line.chars().collect(),
        at: 0,
        limit,
        depth_error: false,
    };
    let value = parser.value(0).map_err(|_| {
        if parser.depth_error {
            ParseError::Depth
        } else {
            ParseError::Invalid
        }
    })?;
    parser.space();
    if parser.at == parser.chars.len() {
        Ok(value)
    } else {
        Err(ParseError::Invalid)
    }
}
struct Parser {
    chars: Vec<char>,
    at: usize,
    limit: usize,
    depth_error: bool,
}
impl Parser {
    fn space(&mut self) {
        while matches!(self.chars.get(self.at), Some(' ' | '\n' | '\r' | '\t')) {
            self.at += 1;
        }
    }
    fn take(&mut self, c: char) -> bool {
        if self.chars.get(self.at) == Some(&c) {
            self.at += 1;
            true
        } else {
            false
        }
    }
    fn literal(&mut self, s: &str) -> bool {
        let n = s.chars().count();
        if self.chars[self.at..].iter().copied().take(n).eq(s.chars()) {
            self.at += n;
            true
        } else {
            false
        }
    }
    fn value(&mut self, depth: usize) -> Result<Value, ()> {
        if depth > self.limit {
            self.depth_error = true;
            return Err(());
        }
        self.space();
        match self.chars.get(self.at).copied() {
            Some('"') => Ok(Value::String(self.string()?)),
            Some('n') if self.literal("null") => Ok(Value::Null),
            Some('t') if self.literal("true") => Ok(Value::Bool(true)),
            Some('f') if self.literal("false") => Ok(Value::Bool(false)),
            Some('N') if self.literal("NaN") => Ok(Value::Number("NaN".into())),
            Some('I') if self.literal("Infinity") => Ok(Value::Number("Infinity".into())),
            Some('[') => {
                self.at += 1;
                self.space();
                let mut a = vec![];
                if self.take(']') {
                    return Ok(Value::Array(a));
                }
                loop {
                    a.push(self.value(depth + 1)?);
                    self.space();
                    if self.take(']') {
                        break;
                    }
                    if !self.take(',') {
                        return Err(());
                    }
                }
                Ok(Value::Array(a))
            }
            Some('{') => {
                self.at += 1;
                self.space();
                let mut o: Vec<(Vec<u32>, Value)> = vec![];
                if self.take('}') {
                    return Ok(Value::Object(o));
                }
                loop {
                    self.space();
                    let k = self.string()?;
                    self.space();
                    if !self.take(':') {
                        return Err(());
                    }
                    let v = self.value(depth + 1)?;
                    // Python dict retains first insertion order, last duplicate value.
                    if let Some((_, old)) = o.iter_mut().find(|(key, _)| key == &k) {
                        *old = v;
                    } else {
                        o.push((k, v));
                    }
                    self.space();
                    if self.take('}') {
                        break;
                    }
                    if !self.take(',') {
                        return Err(());
                    }
                }
                Ok(Value::Object(o))
            }
            Some('-' | '0'..='9') => self.number(),
            _ => Err(()),
        }
    }
    fn hex4(&mut self) -> Result<u32, ()> {
        let mut v = 0;
        for _ in 0..4 {
            v = v * 16
                + self
                    .chars
                    .get(self.at)
                    .and_then(|c| c.to_digit(16))
                    .ok_or(())?;
            self.at += 1;
        }
        Ok(v)
    }
    fn string(&mut self) -> Result<Vec<u32>, ()> {
        if !self.take('"') {
            return Err(());
        }
        let mut s = vec![];
        loop {
            let c = *self.chars.get(self.at).ok_or(())?;
            self.at += 1;
            match c {
                '"' => return Ok(s),
                '\\' => {
                    let e = *self.chars.get(self.at).ok_or(())?;
                    self.at += 1;
                    let mut v = match e {
                        '"' => 34,
                        '\\' => 92,
                        '/' => 47,
                        'b' => 8,
                        'f' => 12,
                        'n' => 10,
                        'r' => 13,
                        't' => 9,
                        'u' => self.hex4()?,
                        _ => return Err(()),
                    };
                    if (0xd800..=0xdbff).contains(&v)
                        && self.chars.get(self.at..self.at + 2) == Some(&['\\', 'u'])
                    {
                        let before = self.at;
                        self.at += 2;
                        let low = self.hex4()?;
                        if (0xdc00..=0xdfff).contains(&low) {
                            v = 0x10000 + ((v - 0xd800) << 10) + low - 0xdc00;
                        } else {
                            self.at = before;
                        }
                    }
                    s.push(v);
                }
                c if c < ' ' => return Err(()),
                c => s.push(u32::from(c)),
            }
        }
    }
    fn number(&mut self) -> Result<Value, ()> {
        let start = self.at;
        self.take('-');
        if self.literal("Infinity") {
            return Ok(Value::Number("-Infinity".into()));
        }
        if !self.take('0') {
            if !matches!(self.chars.get(self.at), Some('1'..='9')) {
                return Err(());
            }
            while matches!(self.chars.get(self.at), Some('0'..='9')) {
                self.at += 1;
            }
        }
        if self.take('.') {
            if !matches!(self.chars.get(self.at), Some('0'..='9')) {
                return Err(());
            }
            while matches!(self.chars.get(self.at), Some('0'..='9')) {
                self.at += 1;
            }
        }
        if self.take('e') || self.take('E') {
            if !self.take('+') {
                self.take('-');
            }
            if !matches!(self.chars.get(self.at), Some('0'..='9')) {
                return Err(());
            }
            while matches!(self.chars.get(self.at), Some('0'..='9')) {
                self.at += 1;
            }
        }
        let number: String = self.chars[start..self.at].iter().collect();
        // Python 3.9's security backport limits decimal int conversion to 4300
        // digits by default. Floats have no such decimal-int conversion.
        if !number.contains(['.', 'e', 'E']) && number.trim_start_matches('-').len() > 4300 {
            return Err(());
        }
        Ok(Value::Number(number))
    }
}
