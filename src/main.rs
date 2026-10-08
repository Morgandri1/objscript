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
use serde_json::json;

pub const USAGE: &str = "usage: objscript <catalog|check|run> <script.json> [--dep PATH=FILE]... [--args JSON]";

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
    let mut i = 2;
    while i < args.len() {
        let val = args.get(i + 1).ok_or_else(|| format!("{} needs a value", args[i]))?;
        match args[i].as_str() {
            "--dep" => {
                let (p, f) = val.split_once('=').ok_or("--dep needs PATH=FILE")?;
                deps.push((p.to_string(), f.to_string()));
            }
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
        "run" => match run(&program, &mut CliHost, &run_args, Limits::default()) {
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