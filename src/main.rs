//! CLI exposing the four operations an agent needs:
//!   objscript catalog
//!   objscript check <script.json> [--dep PATH=FILE]...
//!   objscript run   <script.json> [--dep PATH=FILE]... [--args JSON]
//!   objscript test  <script.json> [--dep PATH=FILE]...
//! All output is JSON so it can be fed straight back to a model.

use std::process::ExitCode;

use objscript::host::{cli::CliHost, catalog};
use objscript::{
    run,
    Diagnostic, 
    Limits, print, 
    load, compile
};
use std::path::Path;
use serde_json::json;

pub const USAGE: &str = "usage: objscript <catalog|check|run> <script.json> [--dep PATH=FILE]... [--dep-dir DIR]... [--args JSON] [--fuel N] [--max-size N] [--max-host-calls N] [--max-depth N]\n\
the CLI runs trusted scripts, so fuel, size and host-call limits are off unless given; call depth is always limited (default 64)";

/// Every `*.json` in `dir` that declares a module `name` becomes importable by
/// that name (and `name@version` when it has a version).
fn dir_deps(dir: &str) -> Result<Vec<(String, String)>, String> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| format!("{dir}: {e}"))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    let mut out = Vec::new();
    for f in files {
        let file = f.to_string_lossy().to_string();
        let json = load(&file)?;
        if let Some(name) = json.get("name").and_then(|n| n.as_str()) {
            out.push((name.to_string(), file.clone()));
            if let Some(v) = json.get("version").and_then(|v| v.as_u64()) {
                out.push((format!("{name}@{v}"), file));
            }
        }
    }
    Ok(out)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match real_main(&args) {
        Ok(code) => code,
        Err(msg) => {
            eprintln!("error: {msg}");
            ExitCode::from(2)
        }
    }
}

fn real_main(args: &[String]) -> Result<ExitCode, String> {
    let host = CliHost;
    let cmd = args.first().map(String::as_str).unwrap_or("help");
    if cmd == "catalog" {
        print(&catalog(&host));
        return Ok(ExitCode::SUCCESS);
    }
    if !matches!(cmd, "check" | "run" | "test") {
        return Err(USAGE.into());
    }
    let file = args.get(1).ok_or(USAGE)?;

    let mut deps = Vec::new();
    let mut run_args = json!({});
    let mut limits = Limits::unlimited();
    let mut i = 2;
    while i < args.len() {
        let val = args.get(i + 1).ok_or_else(|| format!("{} needs a value", args[i]))?;
        match args[i].as_str() {
            "--dep" => {
                let (p, f) = val.split_once('=').ok_or("--dep needs PATH=FILE")?;
                deps.push((p.to_string(), f.to_string()));
            }
            "--dep-dir" => {
                if !Path::new(val).is_dir() {
                    return Err(format!("--dep-dir: {val} is not a directory"));
                }
                deps.extend(dir_deps(val)?);
            }
            "--fuel" => limits.fuel = Some(val.parse().map_err(|_| "--fuel needs a number")?),
            "--max-size" => limits.max_size = Some(val.parse().map_err(|_| "--max-size needs a number")?),
            "--max-host-calls" => limits.max_host_calls = Some(val.parse().map_err(|_| "--max-host-calls needs a number")?),
            "--max-depth" => limits.max_depth = val.parse().map_err(|_| "--max-depth needs a number")?,
            "--args" => run_args = serde_json::from_str(val).map_err(|e| format!("--args: {e}"))?,
            other => return Err(format!("unknown flag {other}")),
        }
        i += 2;
    }

    let src = load(file)?;
    let program = match compile(&src, &deps, &host) {
        Ok(p) => p,
        Err(diags) => {
            print(&json!({ "ok": false, "errors": diags.iter().map(Diagnostic::to_json).collect::<Vec<_>>() }));
            return Ok(ExitCode::FAILURE);
        }
    };

    match cmd {
        "check" => {
            print(&json!({ "ok": true }));
            Ok(ExitCode::SUCCESS)
        }
        "run" => match run(&program, &mut CliHost, &run_args, limits) {
            Ok(o) => {
                print(&json!({ "ok": true, "value": o.value.to_json(), "fuel_used": o.fuel_used, "host_calls": o.host_calls }));
                Ok(ExitCode::SUCCESS)
            }
            Err(e) => {
                print(&json!({ "ok": false, "error": e.to_json() }));
                Ok(ExitCode::FAILURE)
            }
        },
        _ => panic!("no command!"),
    }
}