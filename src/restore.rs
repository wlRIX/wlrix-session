// SPDX-License-Identifier: GPL-3.0-or-later
//! Reopening the applications from the saved session.
//!
//! The compositor wrote the snapshot, and it is the compositor that puts each window back where
//! it was as it appears. The session's part is only to start the programs: it reads each
//! `[[app]]`'s command from the file the compositor named in the `WLRIX_RESTORE` handshake line
//! and runs it, with the same environment as the desktop's own apps.
//!
//! Each launch gets `DESKTOP_STARTUP_ID=wlrix-restore-<n>`, `n` being the app's place in the
//! file. An X11 application copies it onto its windows as `_NET_STARTUP_ID`, which tells the
//! compositor exactly which saved launch a window belongs to; a Wayland one has no such
//! property, and is matched by its `app_id` and title instead.
//!
//! Only `argv` is read here. The rest of the file is the compositor's, so unknown keys are
//! ignored rather than refused -- the opposite of the compositor's own rule for it.

use std::process::Command;

use serde::Deserialize;

use crate::{apps::App, environment::Environment};

/// What the session needs out of a snapshot.
#[derive(Debug, Default, Deserialize)]
struct SessionFile {
    #[serde(default, rename = "app")]
    apps: Vec<SavedApp>,
}

#[derive(Debug, Deserialize)]
struct SavedApp {
    argv: Vec<String>,
}

/// Start every application in the snapshot at `path`, returning the ones that started.
pub fn start_saved(path: &str, environment: &Environment) -> Vec<App> {
    let file = match std::fs::read_to_string(path)
        .map_err(|err| err.to_string())
        .and_then(|text| toml::from_str::<SessionFile>(&text).map_err(|err| err.to_string()))
    {
        Ok(file) => file,
        Err(err) => {
            warn!("could not read the saved session {path}: {err}");
            return Vec::new();
        }
    };

    // From the home directory, as a launcher would: the session's own working directory is
    // wherever the display manager happened to start it.
    let home = std::env::var_os("HOME");
    let mut started = Vec::new();
    for (index, app) in file.apps.iter().enumerate() {
        let Some((program, args)) = app.argv.split_first() else {
            continue;
        };
        let mut command = Command::new(program);
        command
            .args(args)
            .envs(environment.iter())
            .env("DESKTOP_STARTUP_ID", startup_id(index))
            .stdout(crate::log::child_output())
            .stderr(crate::log::child_output());
        if let Some(home) = &home {
            command.current_dir(home);
        }
        match command.spawn() {
            Ok(child) => {
                info!("reopened {program} (pid {})", child.id());
                started.push(App {
                    name: program.clone(),
                    child,
                });
            }
            // Uninstalled since the last login, most likely. Its windows stay unclaimed and
            // the compositor stops waiting for them on its own.
            Err(err) => warn!("could not reopen {program}: {err}"),
        }
    }
    started
}

/// The startup id for the `index`th app in the file. The compositor parses it back; see
/// `STARTUP_ID_PREFIX` in `wlrix-compositor/src/session.rs`.
fn startup_id(index: usize) -> String {
    format!("wlrix-restore-{index}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_commands_are_read() {
        let file: SessionFile = toml::from_str(
            r#"
version = 1

[[app]]
argv = ["wlrix-writer", "/home/v/notes.txt"]
desktop_id = "com.wlrix.writer"

[[app.window]]
app_id = "com.wlrix.writer"
offset = [10, 20]
size = [640, 480]
stack = 3

[app.window.output]
name = "DP-1"

[[app]]
argv = ["foot"]
"#,
        )
        .expect("parse");
        let argvs: Vec<&[String]> = file.apps.iter().map(|app| app.argv.as_slice()).collect();
        assert_eq!(
            argvs,
            [&["wlrix-writer", "/home/v/notes.txt"][..], &["foot"][..]]
        );
    }

    #[test]
    fn startup_ids_count_from_zero() {
        assert_eq!(startup_id(0), "wlrix-restore-0");
        assert_eq!(startup_id(12), "wlrix-restore-12");
    }
}
