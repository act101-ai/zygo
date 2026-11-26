//! The Responses SSE that zygo's encoder makes from Messages SSE (node PC-12: the Codex premise part
//! `scripts/premises/codex/encoder.sh` serves these bytes to the real Codex).
//!
//! Usage: `responses_encode <request.json> <provider> <upstream_model> <client_model>`
//! stdin: Messages SSE bytes. stdout: every event of `Encoder::push(<all stdin>)`, then of `finish()`, as
//! `SseEvent::to_bytes`. stderr: one line per `adjustments()` code. Exit 2 with one line on a wrong argument count or
//! an unreadable or non-JSON `<request.json>`; exit 1 with one line when stdin or stdout fails.

use std::io::{Read, Write};
use std::process::ExitCode;

use serde_json::Value;
use zygo_proxy::translate::responses_client::{Calls, Encoder};
use zygo_proxy::translate::Target;

const USAGE: &str = "usage: responses_encode <request.json> <provider> <upstream_model> <client_model>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [req, provider, upstream_model, model] = args.as_slice() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let req: Value = match std::fs::read(req).map_err(|e| e.to_string()).and_then(|b| {
        serde_json::from_slice(&b).map_err(|e| e.to_string())
    }) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("responses_encode: {req}: {e}");
            return ExitCode::from(2);
        }
    };
    let t = Target { provider: provider.clone(), upstream_model: upstream_model.clone() };
    let mut enc = Encoder::new(model, &t, Calls::of(&req));
    let mut input = Vec::new();
    if let Err(e) = std::io::stdin().read_to_end(&mut input) {
        eprintln!("responses_encode: stdin: {e}");
        return ExitCode::from(1);
    }
    let mut events = enc.push(&input);
    events.extend(enc.finish());
    let mut out = std::io::stdout().lock();
    for ev in &events {
        if let Err(e) = out.write_all(&ev.to_bytes()) {
            eprintln!("responses_encode: stdout: {e}");
            return ExitCode::from(1);
        }
    }
    if let Err(e) = out.flush() {
        eprintln!("responses_encode: stdout: {e}");
        return ExitCode::from(1);
    }
    for a in enc.adjustments() {
        eprintln!("{}", a.0);
    }
    ExitCode::SUCCESS
}
