// SPDX-License-Identifier: GPL-3.0-or-later
//! The desktop's own apps, started once the compositor is up.
//!
//! These are ordinary Wayland clients -- the toolchest and desks are normal windows in
//! IRIX, not shell components -- so there is nothing special about starting them beyond
//! doing it after the compositor exists and with its environment. `wlrix-desktop` is the
//! exception in kind, being a layer-shell background surface rather than a window, but it
//! is started exactly the same way.
//!
//! An app that is missing or fails to start is reported and skipped. A desktop with no
//! toolchest is poor, but better than no desktop at all, and the compositor is still
//! there to launch things from.

use std::process::{Child, Command};

use crate::{config::AppSpec, environment::Environment};

/// What the session starts when there is no config file to say otherwise.
///
/// The desktop comes first so its background surface is up before any window can open over
/// it. Nothing depends on that ordering -- a layer surface goes below windows whenever it
/// arrives -- but starting the backdrop last would show a bare desktop for a moment.
///
/// `wlrix-idle` draws nothing at all, so where it sits is free; it goes last because a
/// session that comes up without it is merely one that never blanks, while a session that
/// comes up without a toolchest has no way to start anything.
pub const DEFAULT_APPS: &[&str] = &[
    "wlrix-desktop",
    "wlrix-toolchest",
    "wlrix-desks",
    "wlrix-idle",
];

pub struct App {
    pub name: String,
    pub child: Child,
}

/// Start each app, skipping any that will not run.
pub fn start_all(apps: &[AppSpec], environment: &Environment) -> Vec<App> {
    let mut started = Vec::new();
    for app in apps {
        match Command::new(&app.command)
            .args(&app.args)
            .envs(environment.iter())
            // Into the session log: an app that fails on startup says why on its own
            // stderr, and on a TTY that would go to the console the compositor covers.
            .stdout(crate::log::child_output())
            .stderr(crate::log::child_output())
            .spawn()
        {
            Ok(child) => {
                info!("started {} (pid {})", app.command, child.id());
                started.push(App {
                    name: app.command.clone(),
                    child,
                });
            }
            Err(err) => {
                warn!("could not start {}: {err}", app.command);
            }
        }
    }
    started
}

/// Ask every app to exit, then wait for it.
///
/// Called when the compositor has gone: the apps' windows died with it, and leaving
/// them running would strand processes with no display to draw on.
pub fn stop_all(apps: Vec<App>) {
    for mut app in apps {
        // Already gone is the common case -- their Wayland connection just dropped.
        match app.child.try_wait() {
            Ok(Some(_)) => continue,
            Ok(None) => {}
            Err(err) => {
                warn!("could not check {}: {err}", app.name);
                continue;
            }
        }
        if let Err(err) = app.child.kill() {
            warn!("could not stop {}: {err}", app.name);
            continue;
        }
        let _ = app.child.wait();
        info!("stopped {}", app.name);
    }
}
