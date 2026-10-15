//! Opening a folder in the system file manager.

use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

use super::Os;

/// The command that opens `dir` on `os`.
#[must_use]
pub fn open_command(os: Os) -> &'static str {
    match os {
        Os::Windows => "explorer",
        Os::MacOs => "open",
        Os::Linux => "xdg-open",
    }
}

/// Opens `dir` (created if missing) in the file manager without waiting for it.
pub fn open_folder(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    Command::new(open_command(Os::current()))
        .arg(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(drop)
}
