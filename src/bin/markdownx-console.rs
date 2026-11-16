//! Console entry point for the Windows GUI executable.
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let Some(executable) = std::env::current_exe().ok().map(|path| path.with_file_name(if cfg!(windows) { "markdownx.exe" } else { "markdownx" })) else {
        eprintln!("MarkdownX: cannot locate the application");
        return ExitCode::FAILURE;
    };
    match Command::new(executable).args(std::env::args_os().skip(1)).status() {
        Ok(status) => ExitCode::from(status.code().unwrap_or(1).clamp(0, 255) as u8),
        Err(error) => { eprintln!("MarkdownX: cannot start the application: {error}"); ExitCode::FAILURE }
    }
}
