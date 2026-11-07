//! Pure built-in functions. Always in scope, positional args only.
//! Type rules live in `check.rs`; this file is evaluation plus catalog text.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::rc::Rc;
use serde_json::Value as J;

use crate::{Diagnostic, Host, Program, check_program};
use crate::parse::parse_module;
use crate::types::value::Value;

pub mod ext;

/// `{"rec": {...}}` of strings -> pairs; `null` (omitted) -> nothing.
pub fn string_pairs(v: &Value) -> Vec<(String, String)> {
    match v {
        Value::Rec(m) => m.iter().map(|(k, v)| (k.clone(), v.render())).collect(),
        _ => Vec::new(),
    }
}

pub fn load(file: &str) -> Result<J, String> {
    let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("{file}: invalid JSON: {e}"))
}

pub fn print(v: &J) {
    println!("{}", serde_json::to_string_pretty(v).expect("serializable"));
}

/// (name, signature, description). Feed this to agents as part of the catalog.
pub const BUILTINS: &[(&str, &str, &str)] = &[
    ("add", "(a: N, b: N) -> N", "Add two ints or two floats."),
    ("sub", "(a: N, b: N) -> N", "Subtract b from a."),
    ("mul", "(a: N, b: N) -> N", "Multiply."),
    ("div", "(a: N, b: N) -> N", "Divide. Int division truncates; dividing an int by 0 is an error."),
    ("mod", "(a: N, b: N) -> N", "Euclidean remainder."),
    ("eq", "(a: T, b: T) -> bool", "Structural equality."),
    ("neq", "(a: T, b: T) -> bool", "Structural inequality."),
    ("lt", "(a: N|string, b: same) -> bool", "Less than."),
    ("lte", "(a: N|string, b: same) -> bool", "Less than or equal."),
    ("gt", "(a: N|string, b: same) -> bool", "Greater than."),
    ("gte", "(a: N|string, b: same) -> bool", "Greater than or equal."),
    ("not", "(a: bool) -> bool", "Logical not."),
    ("concat", "(...parts: any) -> string", "Join the display form of every argument into one string."),
    ("to_string", "(a: any) -> string", "Display form of a value."),
    ("len", "(a: string|list<T>|bytes|map<T>) -> int", "Characters in a string, or items in a list, bytes or map."),
    ("push", "(xs: list<T>, x: T) -> list<T>", "New list with x appended."),
    ("unwrap_or", "(x: option<T>, default: T) -> T", "x if it isn't null, otherwise default."),
    ("min", "(a: N, b: N) -> N", "Smaller of two ints or two floats."),
    ("max", "(a: N, b: N) -> N", "Larger of two ints or two floats."),
    ("abs", "(a: N) -> N", "Absolute value."),
    ("exp", "(x: float) -> float", "e^x."),
    ("log", "(x: float) -> float", "Natural logarithm."),
    ("sqrt", "(x: float) -> float", "Square root."),
    ("pow", "(x: float, y: float) -> float", "x to the power y."),
    ("tanh", "(x: float) -> float", "Hyperbolic tangent."),
    ("sin", "(x: float) -> float", "Sine (radians)."),
    ("cos", "(x: float) -> float", "Cosine (radians)."),
    ("floor", "(x: float) -> float", "Round down."),
    ("ceil", "(x: float) -> float", "Round up."),
    ("round", "(x: float) -> float", "Round half away from zero."),
    ("to_float", "(i: int) -> float", "Convert an int to a float."),
    ("to_int", "(x: float) -> int", "Truncate toward zero. Errors on NaN, infinity or overflow."),
    ("pi", "() -> float", "The constant pi."),
    ("inf", "() -> float", "Positive infinity (negate with sub for -inf)."),
    ("nan", "() -> float", "Not a number."),
    ("is_nan", "(x: float) -> bool", "True if x is NaN."),
    ("band", "(a: int, b: int) -> int", "Bitwise and."),
    ("bor", "(a: int, b: int) -> int", "Bitwise or."),
    ("bxor", "(a: int, b: int) -> int", "Bitwise xor."),
    ("shl", "(a: int, n: int) -> int", "Shift left by 0..63 bits."),
    ("shr", "(a: int, n: int) -> int", "Arithmetic shift right by 0..63 bits."),
    ("bytes_new", "(n: int) -> bytes", "n zero bytes."),
    ("read_u8", "(b: bytes, offset: int) -> int", "Read an unsigned byte at a byte offset. Also read_i8, read_u16, read_i16, read_u32, read_i32 (little-endian)."),
    ("read_f32", "(b: bytes, offset: int) -> float", "Read a little-endian f32 at a byte offset, widened to float. Also read_f64."),
    ("write_u8", "(b: bytes, offset: int, v: int) -> bytes", "New bytes with v written at offset. Also write_i8, write_u16, write_i16, write_u32, write_i32 (little-endian)."),
    ("write_f32", "(b: bytes, offset: int, v: float) -> bytes", "New bytes with an f32 written at offset. Also write_f64."),
    ("concat_bytes", "(a: bytes, b: bytes) -> bytes", "Join two byte strings."),
    ("bytes_from_list", "(xs: list<int>) -> bytes", "Pack ints 0..=255 into bytes."),
    ("bytes_to_list", "(b: bytes) -> list<int>", "Unpack bytes into ints."),
    ("to_bytes", "(s: string) -> bytes", "UTF-8 encode."),
    ("from_utf8", "(b: bytes) -> string", "UTF-8 decode. Errors on invalid input."),
    ("from_utf8_lossy", "(b: bytes) -> string", "UTF-8 decode, replacing invalid sequences with U+FFFD."),
    ("to_hex", "(b: bytes) -> string", "Lowercase hex."),
    ("from_hex", "(s: string) -> bytes", "Decode hex; whitespace is ignored."),
    ("to_base64", "(b: bytes) -> string", "Standard base64 with padding."),
    ("from_base64", "(s: string) -> bytes", "Decode standard base64; whitespace is ignored. This is how a module embeds binary data such as weights."),
    ("split", "(s: string, sep: string) -> list<string>", "Split on a separator; an empty separator splits into characters."),
    ("join", "(xs: list<string>, sep: string) -> string", "Join strings with a separator."),
    ("contains", "(s: string, sub: string) -> bool", "Substring test."),
    ("starts_with", "(s: string, prefix: string) -> bool", "Prefix test."),
    ("ends_with", "(s: string, suffix: string) -> bool", "Suffix test."),
    ("replace", "(s: string, from: string, to: string) -> string", "Replace every occurrence."),
    ("trim", "(s: string) -> string", "Strip surrounding whitespace."),
    ("upper", "(s: string) -> string", "Uppercase."),
    ("lower", "(s: string) -> string", "Lowercase."),
    ("index_of", "(s: string, sub: string) -> int", "Character index of the first match, or -1."),
    ("codepoint", "(s: string) -> int", "Unicode codepoint of the first character."),
    ("from_codepoint", "(i: int) -> string", "One-character string for a codepoint."),
    ("chars", "(s: string) -> list<string>", "Split into characters."),
    ("parse_int", "(s: string) -> option<int>", "Parse an int, or null."),
    ("parse_float", "(s: string) -> option<float>", "Parse a float, or null."),
    ("regex_test", "(s: string, pattern: string) -> bool", "Does the pattern match anywhere? Patterns use Rust regex syntax plus lookahead/lookbehind and backreferences (fancy-regex), including \\p{L} classes."),
    ("regex_find", "(s: string, pattern: string) -> option<string>", "First match, or null."),
    ("regex_find_all", "(s: string, pattern: string) -> list<string>", "Every non-overlapping match, left to right."),
    ("regex_captures", "(s: string, pattern: string) -> option<list<option<string>>>", "Groups of the first match (index 0 is the whole match; unmatched groups are null), or null."),
    ("regex_replace", "(s: string, pattern: string, replacement: string) -> string", "Replace every match. The replacement may use $1, ${name} and $$."),
    ("regex_split", "(s: string, pattern: string) -> list<string>", "Split around every match."),
    ("keys", "(m: map<T>) -> list<string>", "Keys of a map, sorted."),
    ("has", "(m: map<T>, key: string) -> bool", "Does the map contain the key?"),
    ("range", "(n: int) -> list<int>", "[0, 1, ..., n-1]."),
    ("repeat", "(x: T, n: int) -> list<T>", "A list of n copies of x."),
    ("reverse", "(xs: list<T>) -> list<T>", "Reversed copy."),
    ("sort", "(xs: list<int|float|string>) -> same", "Ascending copy."),
    ("concat_lists", "(a: list<T>, b: list<T>) -> list<T>", "Join two lists."),
    ("slice", "(x: string|list<T>|bytes, from: int, to: int) -> same", "Half-open range; negative indexes count from the end; out-of-range bounds are clamped."),
];

pub fn is_builtin(name: &str) -> bool {
    BUILTINS.iter().any(|(n, _, _)| *n == name) || ext::mono_sig(name).is_some()
}

fn overflow() -> String {
    "integer overflow".into()
}

pub fn call(name: &str, args: &[Value]) -> Result<Value, String> {
    use Value::*;
    match (name, args) {
        ("add", [Int(a), Int(b)]) => a.checked_add(*b).map(Int).ok_or_else(overflow),
        ("add", [Float(a), Float(b)]) => Ok(Float(a + b)),
        ("sub", [Int(a), Int(b)]) => a.checked_sub(*b).map(Int).ok_or_else(overflow),
        ("sub", [Float(a), Float(b)]) => Ok(Float(a - b)),
        ("mul", [Int(a), Int(b)]) => a.checked_mul(*b).map(Int).ok_or_else(overflow),
        ("mul", [Float(a), Float(b)]) => Ok(Float(a * b)),
        ("div" | "mod", [Int(_), Int(0)]) => Err("division by zero".into()),
        ("div", [Int(a), Int(b)]) => a.checked_div(*b).map(Int).ok_or_else(overflow),
        ("div", [Float(a), Float(b)]) => Ok(Float(a / b)),
        ("mod", [Int(a), Int(b)]) => a.checked_rem_euclid(*b).map(Int).ok_or_else(overflow),
        ("mod", [Float(a), Float(b)]) => Ok(Float(a.rem_euclid(*b))),
        ("eq", [a, b]) => Ok(Bool(a.equals(b))),
        ("neq", [a, b]) => Ok(Bool(!a.equals(b))),
        ("lt" | "lte" | "gt" | "gte", [a, b]) => {
            let ord = compare(a, b)?;
            Ok(Bool(match name {
                "lt" => ord.is_lt(),
                "lte" => ord.is_le(),
                "gt" => ord.is_gt(),
                _ => ord.is_ge(),
            }))
        }
        ("not", [Bool(b)]) => Ok(Bool(!b)),
        ("concat", parts) => Ok(Value::str(parts.iter().map(Value::render).collect::<String>())),
        ("to_string", [x]) => Ok(Value::str(x.render())),
        ("len", [Str(s)]) => Ok(Int(s.chars().count() as i64)),
        ("len", [List(xs)]) => Ok(Int(xs.len() as i64)),
        ("len", [Bytes(b)]) => Ok(Int(b.len() as i64)),
        ("len", [Rec(m)]) => Ok(Int(m.len() as i64)),
        ("push", [List(xs), x]) => {
            let mut out = (**xs).clone();
            out.push(x.clone());
            Ok(List(Rc::new(out)))
        }
        ("unwrap_or", [Null, default]) => Ok(default.clone()),
        ("unwrap_or", [x, _]) => Ok(x.clone()),
        _ => ext::call(name, args).unwrap_or_else(|| Err(format!("internal error: bad arguments to built-in {name}"))),
    }
}

fn compare(a: &Value, b: &Value) -> Result<Ordering, String> {
    match (a, b) {
        (Value::Int(a), Value::Int(b)) => Ok(a.cmp(b)),
        (Value::Float(a), Value::Float(b)) => a.partial_cmp(b).ok_or_else(|| "cannot compare NaN".into()),
        (Value::Str(a), Value::Str(b)) => Ok(a.cmp(b)),
        _ => Err("internal error: incomparable values".into()),
    }
}

pub fn compile(src: &J, deps: &[(String, String)], host: &dyn Host) -> Result<Program, Vec<Diagnostic>> {
    let entry = parse_module(src).map_err(|d| vec![d])?;
    let mut modules = HashMap::new();
    for (path, file) in deps {
        let json = load(file).map_err(|m| vec![Diagnostic::new(path.as_str(), "load_error", m)])?;
        let m = parse_module(&json).map_err(|mut d| {
            d.path = format!("{path}#{}", d.path);
            vec![d]
        })?;
        modules.insert(path.clone(), m);
    }
    check_program(entry, modules, host)
}
