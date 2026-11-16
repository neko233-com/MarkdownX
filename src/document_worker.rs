//! Run a document parser on a byte snapshot in a bounded Windows process.
use std::io::{Read, Write};
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub(crate) fn run(bytes: Vec<u8>, argument: &str, max_output: u64, timeout: Duration, name: &str) -> Result<Vec<u8>, String> {
    let mut child = Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
        .arg(argument).creation_flags(0x08000000)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|e| e.to_string())?;
    let _job = match crate::pdf_text::constrain(&child) {
        Ok(job) => job,
        Err(error) => { let _ = child.kill(); let _ = child.wait(); return Err(error); }
    };
    let mut stdin = child.stdin.take().ok_or_else(|| format!("{name} worker input unavailable"))?;
    let stdout = child.stdout.take().ok_or_else(|| format!("{name} worker output unavailable"))?;
    let writer = std::thread::spawn(move || stdin.write_all(&bytes));
    let reader = std::thread::spawn(move || {
        let mut output = Vec::new();
        stdout.take(max_output + 1).read_to_end(&mut output).map(|_| output)
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if started.elapsed() < timeout => std::thread::sleep(Duration::from_millis(50)),
            Ok(None) => { let _ = child.kill(); let _ = child.wait(); break Err(format!("{name} preview timed out")); }
            Err(error) => { let _ = child.kill(); let _ = child.wait(); break Err(error.to_string()); }
        }
    };
    let written = writer.join().map_err(|_| format!("{name} worker input failed"))?;
    let output = reader.join().map_err(|_| format!("{name} worker output failed"))?.map_err(|e| e.to_string())?;
    if !status?.success() || written.is_err() { return Err(format!("{name} preview failed; the file may be protected, damaged or unsupported")); }
    if output.len() as u64 > max_output { return Err(format!("{name} preview is too large")); }
    Ok(output)
}
