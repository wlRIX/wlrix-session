// SPDX-License-Identifier: GPL-3.0-or-later
//! Starting the compositor and learning what it decided.
//!
//! The compositor picks its own Wayland socket name, and XWayland picks its own display
//! number. Apps need both, and a process cannot alter a sibling's environment after it
//! has started -- so the session waits to be told before starting anything else.
//!
//! The compositor reports these on stdout as `KEY=VALUE` lines when
//! `WLRIX_SESSION_HANDSHAKE=1` is set; its logs go to stderr and are passed straight
//! through to ours.

use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, RecvTimeoutError},
    time::{Duration, Instant},
};

use crate::environment::Environment;

/// How long to wait for the compositor to say it is up before giving up on it.
///
/// Generous: on a cold boot the first run has a GPU and DRM to bring up.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(20);

/// How long to wait for XWayland after the compositor is otherwise up.
///
/// Short, and not fatal: a session without X11 is worth having, and a missing `DISPLAY`
/// is better than refusing to start the desktop.
const XWAYLAND_TIMEOUT: Duration = Duration::from_secs(10);

pub struct Compositor {
    pub child: Child,
    /// What the compositor reported, ready to hand to apps.
    pub environment: Environment,
}

/// Start the compositor and wait until it is usable.
pub fn start(command: &str, environment: &Environment) -> Result<Compositor, String> {
    let mut child = Command::new(command)
        .envs(environment.iter())
        // Turns on the `KEY=VALUE` reporting this function reads.
        .env("WLRIX_SESSION_HANDSHAKE", "1")
        .stdout(Stdio::piped())
        // Logs pass through untouched, so the session's output is the compositor's.
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|err| format!("could not start {command}: {err}"))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "compositor stdout was not captured".to_string())?;

    // Read on a thread: the handshake has to be waitable with a timeout, and a plain
    // read on a pipe blocks forever if the compositor dies without saying anything.
    // The thread also keeps draining afterwards, so the compositor never blocks on a
    // full pipe.
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            match line.split_once('=') {
                Some((key, value)) if is_handshake_key(key) => {
                    // A closed channel just means the session stopped listening.
                    if sender.send((key.to_string(), value.to_string())).is_err() {
                        break;
                    }
                }
                // Anything else on stdout is the compositor talking to a human.
                _ => println!("{line}"),
            }
        }
    });

    let mut environment = environment.clone();
    match collect(&receiver, &mut environment, &mut child) {
        Ok(()) => Ok(Compositor { child, environment }),
        Err(err) => {
            // Do not leave a half-started compositor behind.
            let _ = child.kill();
            let _ = child.wait();
            Err(err)
        }
    }
}

/// Keys the compositor is allowed to set. Anything else on stdout is passed through as
/// output rather than trusted as configuration.
fn is_handshake_key(key: &str) -> bool {
    matches!(key, "WAYLAND_DISPLAY" | "DISPLAY")
}

/// Wait for the compositor to report its socket, and then for XWayland.
fn collect(
    receiver: &Receiver<(String, String)>,
    environment: &mut Environment,
    child: &mut Child,
) -> Result<(), String> {
    let deadline = Instant::now() + STARTUP_TIMEOUT;

    // `WAYLAND_DISPLAY` is the one that matters: without it there is no session.
    while environment.get("WAYLAND_DISPLAY").is_none() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match receiver.recv_timeout(remaining) {
            Ok((key, value)) => environment.set(&key, &value),
            Err(RecvTimeoutError::Timeout) => {
                return Err("compositor did not come up in time".to_string());
            }
            // The reader thread ended, so the compositor's stdout closed: it exited.
            Err(RecvTimeoutError::Disconnected) => {
                let status = child.try_wait().ok().flatten();
                return Err(match status {
                    Some(status) => format!("compositor exited during startup ({status})"),
                    None => "compositor closed its output during startup".to_string(),
                });
            }
        }
    }

    // XWayland is worth waiting for -- an app started without `DISPLAY` cannot be given
    // one later -- but not worth failing over.
    if environment.get("DISPLAY").is_none() {
        match receiver.recv_timeout(XWAYLAND_TIMEOUT) {
            Ok((key, value)) => environment.set(&key, &value),
            Err(_) => {
                eprintln!(
                    "wlrix-session: no DISPLAY from the compositor; X11 applications \
                     will not run this session"
                );
            }
        }
    }

    Ok(())
}
