//! Reference extension used by the extension tests and as a small protocol
//! example. It serves any suffix passed with `--suffixes` and turns the file
//! into Markdown; `textDocument/save` writes the Markdown back verbatim.
//!
//! Failure modes are selected with `--mode` so the host tests can exercise
//! every rejection path without a second fixture:
//! wrong-version, no-suffixes, duplicate-suffixes, extra-suffix,
//! uppercase-suffix, open-error, wrong-format, no-text, large-text,
//! exit-on-open, notify-open, save-false, save-error.
use std::io::{BufReader, Write};
use fdl_core::extension_host::MAX_DOCUMENT_BYTES;
use fdl_core::extension_rpc::{read_message, write_message};
use serde_json::{Value, json};

fn argument<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().position(|arg| arg == name).and_then(|index| args.get(index + 1)).map(String::as_str)
}

fn suffix_list(value: Option<&str>) -> Vec<String> {
    value.map(|list| list.split(',').filter(|s| !s.is_empty()).map(str::to_owned).collect()).unwrap_or_default()
}

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let suffixes = suffix_list(argument(&args, "--suffixes"));
    let mut advertised = suffix_list(argument(&args, "--advertise"));
    if advertised.is_empty() { advertised = suffixes.clone(); }
    let silent = args.iter().any(|arg| arg == "--silent");
    let sleep = argument(&args, "--sleep-ms").and_then(|value| value.parse::<u64>().ok()).unwrap_or(0);
    let mode = argument(&args, "--mode").unwrap_or("");
    let mut reader = BufReader::new(std::io::stdin().lock());
    let mut writer = std::io::stdout().lock();
    loop {
        let Ok(message) = read_message(&mut reader) else { break };
        // Responses to requests the fixture sent are not requests themselves.
        if message.get("method").is_none() { continue; }
        if silent { continue; }
        let id = message.get("id").cloned().unwrap_or(Value::Null);
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let response = match method {
            "initialize" => {
                let mut result = json!({
                    "protocolVersion": params.get("protocolVersion").cloned().unwrap_or(json!(1)),
                    "name": "MarkdownX fixture",
                    "suffixes": advertised.clone(),
                });
                match mode {
                    "wrong-version" => result["protocolVersion"] = json!(2),
                    "no-suffixes" => { result.as_object_mut().unwrap().remove("suffixes"); }
                    "duplicate-suffixes" => if let Some(first) = advertised.first() { result["suffixes"] = json!([first, first]); },
                    "extra-suffix" => { let mut list = advertised.clone(); list.push(".extra".into()); result["suffixes"] = json!(list); }
                    "uppercase-suffix" => result["suffixes"] = json!(advertised.iter().map(|s| s.to_ascii_uppercase()).collect::<Vec<_>>()),
                    _ => {}
                }
                Ok(result)
            }
            "textDocument/open" => {
                if sleep > 0 { std::thread::sleep(std::time::Duration::from_millis(sleep)); }
                match mode {
                    "exit-on-open" => std::process::exit(0),
                    "open-error" => Err(json!({"code": -32001, "message": "cannot read document"})),
                    "wrong-format" => Ok(json!({"format": "html", "text": "<p>x</p>"})),
                    "no-text" => Ok(json!({"format": "markdown"})),
                    "large-text" => Ok(json!({"format": "markdown", "text": "x".repeat(MAX_DOCUMENT_BYTES + 1)})),
                    "notify-open" => {
                        write_message(&mut writer, &json!({"jsonrpc": "2.0", "method": "window/logMessage", "params": {"message": "working"}})).unwrap();
                        fixture_open(&params)
                    }
                    _ => fixture_open(&params),
                }
            }
            "textDocument/save" => match mode {
                "save-false" => Ok(json!({"ok": false})),
                "save-error" => Err(json!({"code": -32002, "message": "cannot save document"})),
                _ => match (params.get("path").and_then(Value::as_str), params.get("text").and_then(Value::as_str)) {
                    (Some(path), Some(text)) if std::fs::write(path, text).is_ok() => Ok(json!({"ok": true})),
                    _ => Err(json!({"code": -32002, "message": "cannot save document"})),
                },
            },
            "shutdown" => Ok(Value::Null),
            _ => Err(json!({"code": -32601, "message": "method not found"})),
        };
        let message = match response {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err(error) => json!({"jsonrpc": "2.0", "id": id, "error": error}),
        };
        if write_message(&mut writer, &message).is_err() { break; }
        let _ = writer.flush();
        if method == "shutdown" { break; }
    }
}

fn fixture_open(params: &Value) -> Result<Value, Value> {
    match params.get("path").and_then(Value::as_str).and_then(|path| std::fs::read_to_string(path).ok()) {
        Some(text) => Ok(json!({"format": "markdown", "text": format!("# Opened by fixture\n\n{text}")})),
        None => Err(json!({"code": -32001, "message": "cannot read document"})),
    }
}
