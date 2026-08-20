//! 最小の JSON。**依存を増やさないために自分で書く。**
//!
//! LSP が使う範囲しか要らない——それは JSON の全部ではあるが、
//! 深い入れ子も巨大な数も来ない。**正しさだけ落とさないようにする。**

#[derive(Clone, Debug, PartialEq)]
pub enum J {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

impl J {
    pub fn get(&self, k: &str) -> Option<&J> {
        match self {
            J::Obj(m) => m.iter().find(|(n, _)| n == k).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn at(&self, i: usize) -> Option<&J> {
        match self {
            J::Arr(a) => a.get(i),
            _ => None,
        }
    }
    pub fn str(&self) -> Option<&str> {
        match self {
            J::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn int(&self) -> Option<i64> {
        match self {
            J::Num(n) => Some(*n as i64),
            _ => None,
        }
    }
    /// 経路で引く。`j.path(&["params","textDocument","uri"])`
    pub fn path(&self, ks: &[&str]) -> Option<&J> {
        let mut cur = self;
        for k in ks {
            cur = cur.get(k)?;
        }
        Some(cur)
    }
}

pub fn obj(pairs: Vec<(&str, J)>) -> J {
    J::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

pub fn s(x: &str) -> J {
    J::Str(x.to_string())
}

pub fn n(x: i64) -> J {
    J::Num(x as f64)
}

// ========== 書き出す ==========

pub fn write(j: &J) -> String {
    let mut out = String::new();
    put(j, &mut out);
    out
}

fn put(j: &J, out: &mut String) {
    match j {
        J::Null => out.push_str("null"),
        J::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        J::Num(x) => {
            if x.fract() == 0.0 && x.abs() < 9e15 {
                out.push_str(&format!("{}", *x as i64));
            } else {
                out.push_str(&format!("{x}"));
            }
        }
        J::Str(x) => put_str(x, out),
        J::Arr(a) => {
            out.push('[');
            for (i, v) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                put(v, out);
            }
            out.push(']');
        }
        J::Obj(m) => {
            out.push('{');
            for (i, (k, v)) in m.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                put_str(k, out);
                out.push(':');
                put(v, out);
            }
            out.push('}');
        }
    }
}

fn put_str(x: &str, out: &mut String) {
    out.push('"');
    for c in x.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

// ========== 読み取る ==========

pub fn parse(src: &str) -> Option<J> {
    let b: Vec<char> = src.chars().collect();
    let mut i = 0;
    let v = val(&b, &mut i)?;
    Some(v)
}

fn ws(b: &[char], i: &mut usize) {
    while *i < b.len() && matches!(b[*i], ' ' | '\t' | '\n' | '\r') {
        *i += 1;
    }
}

fn val(b: &[char], i: &mut usize) -> Option<J> {
    ws(b, i);
    match *b.get(*i)? {
        'n' => lit(b, i, "null", J::Null),
        't' => lit(b, i, "true", J::Bool(true)),
        'f' => lit(b, i, "false", J::Bool(false)),
        '"' => Some(J::Str(string(b, i)?)),
        '[' => {
            *i += 1;
            let mut a = Vec::new();
            ws(b, i);
            if b.get(*i) == Some(&']') {
                *i += 1;
                return Some(J::Arr(a));
            }
            loop {
                a.push(val(b, i)?);
                ws(b, i);
                match b.get(*i)? {
                    ',' => *i += 1,
                    ']' => {
                        *i += 1;
                        return Some(J::Arr(a));
                    }
                    _ => return None,
                }
            }
        }
        '{' => {
            *i += 1;
            let mut m = Vec::new();
            ws(b, i);
            if b.get(*i) == Some(&'}') {
                *i += 1;
                return Some(J::Obj(m));
            }
            loop {
                ws(b, i);
                let k = string(b, i)?;
                ws(b, i);
                if b.get(*i) != Some(&':') {
                    return None;
                }
                *i += 1;
                m.push((k, val(b, i)?));
                ws(b, i);
                match b.get(*i)? {
                    ',' => *i += 1,
                    '}' => {
                        *i += 1;
                        return Some(J::Obj(m));
                    }
                    _ => return None,
                }
            }
        }
        _ => num(b, i),
    }
}

fn lit(b: &[char], i: &mut usize, w: &str, v: J) -> Option<J> {
    for c in w.chars() {
        if b.get(*i) != Some(&c) {
            return None;
        }
        *i += 1;
    }
    Some(v)
}

fn string(b: &[char], i: &mut usize) -> Option<String> {
    if b.get(*i) != Some(&'"') {
        return None;
    }
    *i += 1;
    let mut out = String::new();
    loop {
        let c = *b.get(*i)?;
        *i += 1;
        match c {
            '"' => return Some(out),
            '\\' => {
                let e = *b.get(*i)?;
                *i += 1;
                match e {
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    'r' => out.push('\r'),
                    'b' => out.push('\u{8}'),
                    'f' => out.push('\u{c}'),
                    '/' => out.push('/'),
                    '"' => out.push('"'),
                    '\\' => out.push('\\'),
                    'u' => {
                        let hi = hex4(b, i)?;
                        // **代用対を組み直す。** LSP は UTF-16 で来る
                        if (0xD800..0xDC00).contains(&hi)
                            && b.get(*i) == Some(&'\\')
                            && b.get(*i + 1) == Some(&'u')
                        {
                            *i += 2;
                            let lo = hex4(b, i)?;
                            if (0xDC00..0xE000).contains(&lo) {
                                let cp = 0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00);
                                out.push(char::from_u32(cp)?);
                                continue;
                            }
                            return None;
                        }
                        out.push(char::from_u32(hi)?);
                    }
                    _ => return None,
                }
            }
            c => out.push(c),
        }
    }
}

fn hex4(b: &[char], i: &mut usize) -> Option<u32> {
    let mut v = 0u32;
    for _ in 0..4 {
        v = v * 16 + b.get(*i)?.to_digit(16)?;
        *i += 1;
    }
    Some(v)
}

fn num(b: &[char], i: &mut usize) -> Option<J> {
    let start = *i;
    if b.get(*i) == Some(&'-') {
        *i += 1;
    }
    while *i < b.len() && (b[*i].is_ascii_digit() || matches!(b[*i], '.' | 'e' | 'E' | '+' | '-')) {
        *i += 1;
    }
    let t: String = b[start..*i].iter().collect();
    t.parse().ok().map(J::Num)
}
