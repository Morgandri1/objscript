//! Math, bytes, string and collection built-ins beyond the core arithmetic set.
//! Pure and deterministic. Monomorphic ones are described by [`mono_sig`] so the
//! checker needs no per-function code; polymorphic ones are special-cased there.

use std::cmp::Ordering;
use std::rc::Rc;

use crate::types::value::Value;
use crate::types::Type;

/// Signature of a monomorphic built-in, if `name` is one.
pub fn mono_sig(name: &str) -> Option<(Vec<Type>, Type)> {
    use Type::{Bool, Bytes, Float, Int, Str};
    let list_str = || Type::list(Str);
    Some(match name {
        // math
        "exp" | "log" | "sqrt" | "tanh" | "sin" | "cos" | "floor" | "ceil" | "round" => (vec![Float], Float),
        "pow" => (vec![Float, Float], Float),
        "to_float" => (vec![Int], Float),
        "to_int" => (vec![Float], Int),
        "pi" | "inf" | "nan" => (vec![], Float),
        "is_nan" => (vec![Float], Bool),
        "band" | "bor" | "bxor" | "shl" | "shr" => (vec![Int, Int], Int),
        // bytes
        "bytes_new" => (vec![Int], Bytes),
        "read_u8" | "read_i8" | "read_u16" | "read_i16" | "read_u32" | "read_i32" => (vec![Bytes, Int], Int),
        "read_f32" | "read_f64" => (vec![Bytes, Int], Float),
        "write_u8" | "write_i8" | "write_u16" | "write_i16" | "write_u32" | "write_i32" => (vec![Bytes, Int, Int], Bytes),
        "write_f32" | "write_f64" => (vec![Bytes, Int, Float], Bytes),
        "concat_bytes" => (vec![Bytes, Bytes], Bytes),
        "bytes_from_list" => (vec![Type::list(Int)], Bytes),
        "bytes_to_list" => (vec![Bytes], Type::list(Int)),
        "to_bytes" => (vec![Str], Bytes),
        "from_utf8" | "from_utf8_lossy" | "to_hex" | "to_base64" => (vec![Bytes], Str),
        "from_hex" | "from_base64" => (vec![Str], Bytes),
        // strings
        "split" => (vec![Str, Str], list_str()),
        "join" => (vec![list_str(), Str], Str),
        "contains" | "starts_with" | "ends_with" => (vec![Str, Str], Bool),
        "replace" => (vec![Str, Str, Str], Str),
        "trim" | "upper" | "lower" => (vec![Str], Str),
        "index_of" => (vec![Str, Str], Int),
        "codepoint" => (vec![Str], Int),
        "from_codepoint" => (vec![Int], Str),
        "chars" => (vec![Str], list_str()),
        "parse_int" => (vec![Str], Type::option(Int)),
        "parse_float" => (vec![Str], Type::option(Float)),
        // maps
        "keys" => (vec![Type::map(Type::Unknown)], list_str()),
        "has" => (vec![Type::map(Type::Unknown), Str], Bool),
        "range" => (vec![Int], Type::list(Int)),
        // regex (fancy-regex syntax: Rust regex plus lookaround and backreferences)
        "regex_test" => (vec![Str, Str], Bool),
        "regex_find" => (vec![Str, Str], Type::option(Str)),
        "regex_find_all" | "regex_split" => (vec![Str, Str], list_str()),
        "regex_replace" => (vec![Str, Str, Str], Str),
        "regex_captures" => (vec![Str, Str], Type::option(Type::list(Type::option(Str)))),
        _ => return None,
    })
}

fn width(name: &str) -> usize {
    match name.rsplit('_').next().unwrap_or("") {
        "u8" | "i8" => 1,
        "u16" | "i16" => 2,
        "u32" | "i32" | "f32" => 4,
        _ => 8,
    }
}

fn span(len: usize, off: i64, n: usize) -> Result<std::ops::Range<usize>, String> {
    let start = usize::try_from(off).map_err(|_| format!("negative byte offset {off}"))?;
    let end = start.checked_add(n).filter(|e| *e <= len);
    end.map(|e| start..e)
        .ok_or_else(|| format!("reading {n} bytes at offset {off} is out of bounds for bytes of length {len}"))
}

fn read(name: &str, b: &[u8], off: i64) -> Result<Value, String> {
    let r = span(b.len(), off, width(name))?;
    let s = &b[r];
    Ok(match name {
        "read_u8" => Value::Int(s[0] as i64),
        "read_i8" => Value::Int(s[0] as i8 as i64),
        "read_u16" => Value::Int(u16::from_le_bytes(s.try_into().unwrap()) as i64),
        "read_i16" => Value::Int(i16::from_le_bytes(s.try_into().unwrap()) as i64),
        "read_u32" => Value::Int(u32::from_le_bytes(s.try_into().unwrap()) as i64),
        "read_i32" => Value::Int(i32::from_le_bytes(s.try_into().unwrap()) as i64),
        "read_f32" => Value::Float(f32::from_le_bytes(s.try_into().unwrap()) as f64),
        _ => Value::Float(f64::from_le_bytes(s.try_into().unwrap())),
    })
}

fn write(name: &str, b: &[u8], off: i64, v: &Value) -> Result<Value, String> {
    let r = span(b.len(), off, width(name))?;
    let mut out = b.to_vec();
    let dst = &mut out[r];
    match (name, v) {
        ("write_u8", Value::Int(x)) => dst.copy_from_slice(&u8::try_from(*x).map_err(|_| format!("{x} does not fit u8"))?.to_le_bytes()),
        ("write_i8", Value::Int(x)) => dst.copy_from_slice(&i8::try_from(*x).map_err(|_| format!("{x} does not fit i8"))?.to_le_bytes()),
        ("write_u16", Value::Int(x)) => dst.copy_from_slice(&u16::try_from(*x).map_err(|_| format!("{x} does not fit u16"))?.to_le_bytes()),
        ("write_i16", Value::Int(x)) => dst.copy_from_slice(&i16::try_from(*x).map_err(|_| format!("{x} does not fit i16"))?.to_le_bytes()),
        ("write_u32", Value::Int(x)) => dst.copy_from_slice(&u32::try_from(*x).map_err(|_| format!("{x} does not fit u32"))?.to_le_bytes()),
        ("write_i32", Value::Int(x)) => dst.copy_from_slice(&i32::try_from(*x).map_err(|_| format!("{x} does not fit i32"))?.to_le_bytes()),
        ("write_f32", Value::Float(x)) => dst.copy_from_slice(&(*x as f32).to_le_bytes()),
        ("write_f64", Value::Float(x)) => dst.copy_from_slice(&x.to_le_bytes()),
        _ => return Err(format!("internal error: bad arguments to built-in {name}")),
    }
    Ok(Value::Bytes(Rc::from(out)))
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn to_base64(b: &[u8]) -> String {
    let mut out = String::with_capacity(b.len().div_ceil(3) * 4);
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn from_base64(s: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return Err(format!("invalid base64 character {:?}", c as char)),
        };
        acc = acc << 6 | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits & 0xff) as u8);
        }
    }
    Ok(out)
}

fn from_hex(s: &str) -> Result<Vec<u8>, String> {
    let digits: Vec<u8> = s
        .bytes()
        .filter(|c| !c.is_ascii_whitespace())
        .map(|c| match c {
            b'0'..=b'9' => Ok(c - b'0'),
            b'a'..=b'f' => Ok(c - b'a' + 10),
            b'A'..=b'F' => Ok(c - b'A' + 10),
            _ => Err(format!("invalid hex character {:?}", c as char)),
        })
        .collect::<Result<_, _>>()?;
    if digits.len() % 2 != 0 {
        return Err("hex string has an odd number of digits".into());
    }
    Ok(digits.chunks(2).map(|p| p[0] << 4 | p[1]).collect())
}

/// Resolve possibly-negative `i`/`j` against `len`, clamped; the range is empty if j < i.
fn bounds(len: usize, i: i64, j: i64) -> (usize, usize) {
    let norm = |x: i64| (if x < 0 { len as i64 + x } else { x }).clamp(0, len as i64) as usize;
    let (a, b) = (norm(i), norm(j));
    (a, b.max(a))
}

fn strs(xs: &[Value]) -> Result<Vec<&str>, String> {
    xs.iter()
        .map(|v| match v {
            Value::Str(s) => Ok(&**s),
            _ => Err("internal error: expected a list of strings".to_string()),
        })
        .collect()
}

fn order(a: &Value, b: &Value) -> Ordering {
    match (a, b) {
        (Value::Int(a), Value::Int(b)) => a.cmp(b),
        (Value::Float(a), Value::Float(b)) => a.total_cmp(b),
        (Value::Str(a), Value::Str(b)) => a.cmp(b),
        _ => Ordering::Equal,
    }
}

thread_local! {
    static REGEXES: std::cell::RefCell<std::collections::HashMap<String, Rc<fancy_regex::Regex>>> = Default::default();
}

fn compile_regex(pat: &str) -> Result<Rc<fancy_regex::Regex>, String> {
    REGEXES.with(|c| {
        if let Some(r) = c.borrow().get(pat) {
            return Ok(r.clone());
        }
        let r = Rc::new(fancy_regex::Regex::new(pat).map_err(|e| format!("invalid regex: {e}"))?);
        c.borrow_mut().insert(pat.to_string(), r.clone());
        Ok(r)
    })
}

fn rx_err(e: fancy_regex::Error) -> String {
    format!("regex error: {e}")
}

fn regex_call(name: &str, s: &str, pat: &str, rep: Option<&str>) -> Result<Value, String> {
    let re = compile_regex(pat)?;
    Ok(match name {
        "regex_test" => Value::Bool(re.is_match(s).map_err(rx_err)?),
        "regex_find" => match re.find(s).map_err(rx_err)? {
            Some(m) => Value::str(m.as_str()),
            None => Value::Null,
        },
        "regex_find_all" => {
            let mut out = Vec::new();
            for m in re.find_iter(s) {
                out.push(Value::str(m.map_err(rx_err)?.as_str()));
            }
            Value::List(Rc::new(out))
        }
        "regex_split" => {
            let mut out = Vec::new();
            let mut last = 0;
            for m in re.find_iter(s) {
                let m = m.map_err(rx_err)?;
                if m.range().is_empty() {
                    continue;
                }
                out.push(Value::str(&s[last..m.start()]));
                last = m.end();
            }
            out.push(Value::str(&s[last..]));
            Value::List(Rc::new(out))
        }
        "regex_replace" => {
            let rep = rep.unwrap_or_default();
            let mut out = String::new();
            let mut last = 0;
            for c in re.captures_iter(s) {
                let c = c.map_err(rx_err)?;
                let m = c.get(0).expect("group 0");
                out.push_str(&s[last..m.start()]);
                c.expand(rep, &mut out);
                last = m.end();
            }
            out.push_str(&s[last..]);
            Value::str(out)
        }
        _ => match re.captures(s).map_err(rx_err)? {
            Some(c) => Value::List(Rc::new(
                (0..c.len()).map(|i| c.get(i).map_or(Value::Null, |m| Value::str(m.as_str()))).collect(),
            )),
            None => Value::Null,
        },
    })
}

/// `None` if `name` isn't handled here.
pub fn call(name: &str, args: &[Value]) -> Option<Result<Value, String>> {
    use Value::*;
    let r = match (name, args) {
        // ---- math ----
        ("exp", [Float(x)]) => Ok(Float(x.exp())),
        ("log", [Float(x)]) => Ok(Float(x.ln())),
        ("sqrt", [Float(x)]) => Ok(Float(x.sqrt())),
        ("tanh", [Float(x)]) => Ok(Float(x.tanh())),
        ("sin", [Float(x)]) => Ok(Float(x.sin())),
        ("cos", [Float(x)]) => Ok(Float(x.cos())),
        ("floor", [Float(x)]) => Ok(Float(x.floor())),
        ("ceil", [Float(x)]) => Ok(Float(x.ceil())),
        ("round", [Float(x)]) => Ok(Float(x.round())),
        ("pow", [Float(a), Float(b)]) => Ok(Float(a.powf(*b))),
        ("to_float", [Int(i)]) => Ok(Float(*i as f64)),
        ("to_int", [Float(x)]) if x.is_finite() && x.abs() < 9.2e18 => Ok(Int(*x as i64)),
        ("to_int", [Float(x)]) => Err(format!("cannot convert {x} to int")),
        ("pi", []) => Ok(Float(std::f64::consts::PI)),
        ("inf", []) => Ok(Float(f64::INFINITY)),
        ("nan", []) => Ok(Float(f64::NAN)),
        ("is_nan", [Float(x)]) => Ok(Bool(x.is_nan())),
        ("band", [Int(a), Int(b)]) => Ok(Int(a & b)),
        ("bor", [Int(a), Int(b)]) => Ok(Int(a | b)),
        ("bxor", [Int(a), Int(b)]) => Ok(Int(a ^ b)),
        ("shl" | "shr", [Int(a), Int(n)]) if !(0..64).contains(n) => Err(format!("shift amount {n} out of range for {a}")),
        ("shl", [Int(a), Int(n)]) => Ok(Int(a.wrapping_shl(*n as u32))),
        ("shr", [Int(a), Int(n)]) => Ok(Int(a >> n)),
        ("min", [Int(a), Int(b)]) => Ok(Int(*a.min(b))),
        ("max", [Int(a), Int(b)]) => Ok(Int(*a.max(b))),
        ("min", [Float(a), Float(b)]) => Ok(Float(a.min(*b))),
        ("max", [Float(a), Float(b)]) => Ok(Float(a.max(*b))),
        ("abs", [Int(a)]) => a.checked_abs().map(Int).ok_or_else(|| "integer overflow".into()),
        ("abs", [Float(a)]) => Ok(Float(a.abs())),

        // ---- bytes ----
        ("bytes_new", [Int(n)]) => usize::try_from(*n).map(|n| Bytes(Rc::from(vec![0u8; n]))).map_err(|_| "negative length".into()),
        (n, [Bytes(b), Int(o)]) if n.starts_with("read_") => read(n, b, *o),
        (n, [Bytes(b), Int(o), v]) if n.starts_with("write_") => write(n, b, *o, v),
        ("concat_bytes", [Bytes(a), Bytes(b)]) => Ok(Bytes(Rc::from([&a[..], &b[..]].concat()))),
        ("bytes_from_list", [List(xs)]) => xs
            .iter()
            .map(|v| match v {
                Int(i) => u8::try_from(*i).map_err(|_| format!("{i} is not a byte (0..=255)")),
                _ => Err("internal error: expected ints".into()),
            })
            .collect::<Result<Vec<u8>, _>>()
            .map(|v| Bytes(Rc::from(v))),
        ("bytes_to_list", [Bytes(b)]) => Ok(List(Rc::new(b.iter().map(|x| Int(*x as i64)).collect()))),
        ("to_bytes", [Str(s)]) => Ok(Bytes(Rc::from(s.as_bytes()))),
        ("from_utf8", [Bytes(b)]) => std::str::from_utf8(b).map(Value::str).map_err(|e| format!("invalid utf-8: {e}")),
        ("from_utf8_lossy", [Bytes(b)]) => Ok(Value::str(String::from_utf8_lossy(b))),
        ("to_hex", [Bytes(b)]) => Ok(Value::str(b.iter().map(|x| format!("{x:02x}")).collect::<String>())),
        ("from_hex", [Str(s)]) => from_hex(s).map(|v| Bytes(Rc::from(v))),
        ("to_base64", [Bytes(b)]) => Ok(Value::str(to_base64(b))),
        ("from_base64", [Str(s)]) => from_base64(s).map(|v| Bytes(Rc::from(v))),

        // ---- strings ----
        ("split", [Str(s), Str(sep)]) if sep.is_empty() => Ok(List(Rc::new(s.chars().map(|c| Value::str(c.to_string())).collect()))),
        ("split", [Str(s), Str(sep)]) => Ok(List(Rc::new(s.split(&**sep).map(Value::str).collect()))),
        ("join", [List(xs), Str(sep)]) => strs(xs).map(|v| Value::str(v.join(sep))),
        ("contains", [Str(s), Str(t)]) => Ok(Bool(s.contains(&**t))),
        ("starts_with", [Str(s), Str(t)]) => Ok(Bool(s.starts_with(&**t))),
        ("ends_with", [Str(s), Str(t)]) => Ok(Bool(s.ends_with(&**t))),
        ("replace", [Str(s), Str(from), Str(_)]) if from.is_empty() => Ok(Str(s.clone())),
        ("replace", [Str(s), Str(from), Str(to)]) => Ok(Value::str(s.replace(&**from, to))),
        ("trim", [Str(s)]) => Ok(Value::str(s.trim())),
        ("upper", [Str(s)]) => Ok(Value::str(s.to_uppercase())),
        ("lower", [Str(s)]) => Ok(Value::str(s.to_lowercase())),
        ("index_of", [Str(s), Str(t)]) => Ok(Int(s.find(&**t).map_or(-1, |b| s[..b].chars().count() as i64))),
        ("codepoint", [Str(s)]) => s.chars().next().map(|c| Int(c as i64)).ok_or_else(|| "codepoint of an empty string".into()),
        ("from_codepoint", [Int(i)]) => u32::try_from(*i)
            .ok()
            .and_then(char::from_u32)
            .map(|c| Value::str(c.to_string()))
            .ok_or_else(|| format!("{i} is not a valid codepoint")),
        ("chars", [Str(s)]) => Ok(List(Rc::new(s.chars().map(|c| Value::str(c.to_string())).collect()))),
        ("parse_int", [Str(s)]) => Ok(s.trim().parse::<i64>().map_or(Null, Int)),
        ("parse_float", [Str(s)]) => Ok(s.trim().parse::<f64>().map_or(Null, Float)),

        // ---- collections ----
        ("keys", [Rec(m)]) => Ok(List(Rc::new(m.keys().map(Value::str).collect()))),
        ("has", [Rec(m), Str(k)]) => Ok(Bool(m.contains_key(&**k))),
        (n, [Str(s), Str(p)]) if n.starts_with("regex_") => regex_call(n, s, p, None),
        ("regex_replace", [Str(s), Str(p), Str(r)]) => regex_call("regex_replace", s, p, Some(r)),
        ("range", [Int(n)]) => Ok(List(Rc::new((0..(*n).max(0)).map(Int).collect()))),
        ("repeat", [x, Int(n)]) if *n >= 0 => Ok(List(Rc::new(vec![x.clone(); *n as usize]))),
        ("repeat", [_, Int(n)]) => Err(format!("negative repeat count {n}")),
        ("reverse", [List(xs)]) => Ok(List(Rc::new(xs.iter().rev().cloned().collect()))),
        ("sort", [List(xs)]) => {
            let mut out = (**xs).clone();
            out.sort_by(order);
            Ok(List(Rc::new(out)))
        }
        ("concat_lists", [List(a), List(b)]) => Ok(List(Rc::new(a.iter().chain(b.iter()).cloned().collect()))),
        ("slice", [Str(s), Int(i), Int(j)]) => {
            let cs: Vec<char> = s.chars().collect();
            let (a, b) = bounds(cs.len(), *i, *j);
            Ok(Value::str(cs[a..b].iter().collect::<String>()))
        }
        ("slice", [List(xs), Int(i), Int(j)]) => {
            let (a, b) = bounds(xs.len(), *i, *j);
            Ok(List(Rc::new(xs[a..b].to_vec())))
        }
        ("slice", [Bytes(x), Int(i), Int(j)]) => {
            let (a, b) = bounds(x.len(), *i, *j);
            Ok(Bytes(Rc::from(&x[a..b])))
        }
        _ => return None,
    };
    Some(r)
}
