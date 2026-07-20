// SPDX-License-Identifier: GPL-3.0-or-later
//! Where the session's output goes.
//!
//! Two destinations. The terminal, as before, which is what you want when running the
//! session by hand. And a file, because on a TTY the console the session is writing to
//! is covered by the compositor the moment it starts: the text is there, but nothing
//! can be read of it, so anything that goes wrong after startup leaves no trace.
//!
//! The apps' own output is captured into the same file. Their stdout and stderr would
//! otherwise go to that same invisible console, which is exactly where the reason an
//! app failed to start would have gone.
//!
//! Truncated at startup: this is a record of the current session, and one that grew
//! across boots would bury it.

use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Mutex, OnceLock},
};

const LOG_NAME: &str = "wlrix-session.log";

/// The log file, once opened. `None` if it could not be opened, in which case the
/// terminal is all there is -- worth carrying on for, not worth failing to start over.
static FILE: OnceLock<Option<Mutex<File>>> = OnceLock::new();

/// Open the log file, truncating it. Returns where it is, if anywhere.
pub fn init() -> Option<PathBuf> {
    let path = std::env::temp_dir().join(LOG_NAME);
    let opened = match create(&path) {
        Ok(file) => Some(Mutex::new(file)),
        Err(err) => {
            eprintln!("wlrix-session: could not open {}: {err}", path.display());
            None
        }
    };
    let stored = FILE.get_or_init(|| opened);
    stored.as_ref().map(|_| path)
}

/// Open for writing, truncating what is there.
///
/// `O_NOFOLLOW` because the path is predictable and lives in a world-writable
/// directory: without it, anyone could leave a symlink there and have the session
/// truncate a file of their choosing.
fn create(path: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    File::options()
        .write(true)
        .create(true)
        .truncate(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
}

/// Somewhere for a child process to write, so its output lands in the log rather than
/// on a console nobody can see.
///
/// Falls back to inheriting ours if there is no log file, which is the best available
/// answer rather than discarding what the child has to say.
pub fn child_output() -> Stdio {
    let Some(Some(file)) = FILE.get() else {
        return Stdio::inherit();
    };
    let Ok(file) = file.lock() else {
        return Stdio::inherit();
    };
    // A duplicate of the same open file, so children append after each other rather
    // than each writing from the start and overwriting.
    match file.try_clone() {
        Ok(clone) => Stdio::from(clone),
        Err(_) => Stdio::inherit(),
    }
}

/// Write a line to the log file, if there is one.
fn to_file(line: &str) {
    let Some(Some(file)) = FILE.get() else {
        return;
    };
    if let Ok(mut file) = file.lock() {
        // Nothing useful to do if the log cannot be written; the terminal already has
        // the message, and failing here would be noise about noise.
        let _ = writeln!(file, "{line}");
        let _ = file.flush();
    }
}

/// An ordinary message: stdout and the log.
pub fn out(args: std::fmt::Arguments) {
    let line = format!("wlrix-session: {args}");
    println!("{line}");
    to_file(&line);
}

/// Something wrong: stderr and the log.
pub fn err(args: std::fmt::Arguments) {
    let line = format!("wlrix-session: {args}");
    eprintln!("{line}");
    to_file(&line);
}

/// Report something that went as expected.
macro_rules! info {
    ($($arg:tt)*) => { $crate::log::out(format_args!($($arg)*)) };
}

/// Report something that did not.
macro_rules! warn {
    ($($arg:tt)*) => { $crate::log::err(format_args!($($arg)*)) };
}
