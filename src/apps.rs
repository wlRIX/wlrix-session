// SPDX-License-Identifier: GPL-3.0-or-later
//! The desktop's own apps, started once the compositor is up.
//!
//! These are ordinary Wayland clients -- the toolchest and desks are normal windows in
//! IRIX, not shell components -- so there is nothing special about starting them beyond
//! doing it after the compositor exists and with its environment.
//!
//! An app that is missing or fails to start is reported and skipped. A desktop with no
//! toolchest is poor, but better than no desktop at all, and the compositor is still
//! there to launch things from.

use std::process::{Child, Command};

use crate::{config::AppSpec, environment::Environment};

/// What the session starts when there is no config file to say otherwise.
pub const DEFAULT_APPS: &[&str] = &["wlrix-toolchest", "wlrix-desks"];

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
            .spawn()
        {
            Ok(child) => {
                println!(
                    "wlrix-session: started {} (pid {})",
                    app.command,
                    child.id()
                );
                started.push(App {
                    name: app.command.clone(),
                    child,
                });
            }
            Err(err) => {
                eprintln!("wlrix-session: could not start {}: {err}", app.command);
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
                eprintln!("wlrix-session: could not check {}: {err}", app.name);
                continue;
            }
        }
        if let Err(err) = app.child.kill() {
            eprintln!("wlrix-session: could not stop {}: {err}", app.name);
            continue;
        }
        let _ = app.child.wait();
        println!("wlrix-session: stopped {}", app.name);
    }
}
