// SPDX-License-Identifier: GPL-3.0-or-later
//! wlRIX session manager.
//!
//! Starts the compositor, waits to learn the environment only it can decide, starts the
//! desktop's apps with that environment, and stays until the compositor exits.
//!
//! The ordering is the whole point. `WAYLAND_DISPLAY` and `DISPLAY` are not known until
//! the compositor and XWayland are running, and a process cannot be given them after it
//! has started -- so apps wait, rather than starting alongside and finding an empty
//! `DISPLAY`.
//!
//! The compositor is the session: when it exits, the session tears down and exits with
//! its status.

mod apps;
mod compositor;
mod environment;
mod signals;

use environment::Environment;

/// The compositor to run, unless `--compositor` says otherwise.
const DEFAULT_COMPOSITOR: &str = "wlrix-compositor";

struct Args {
    compositor: String,
    apps: Vec<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut compositor = DEFAULT_COMPOSITOR.to_string();
    let mut apps: Vec<String> = apps::DEFAULT_APPS
        .iter()
        .map(|app| app.to_string())
        .collect();
    let mut replaced_apps = false;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--compositor" => {
                compositor = args
                    .next()
                    .ok_or_else(|| "--compositor needs a command".to_string())?;
            }
            // Repeatable, and the first one clears the defaults so `--app foo` means
            // "just foo" rather than "the usual list and also foo".
            "--app" => {
                let app = args
                    .next()
                    .ok_or_else(|| "--app needs a command".to_string())?;
                if !replaced_apps {
                    apps.clear();
                    replaced_apps = true;
                }
                apps.push(app);
            }
            "--no-apps" => {
                apps.clear();
                replaced_apps = true;
            }
            "--help" | "-h" => {
                println!(
                    "wlrix-session {}\n\n\
                     Usage: wlrix-session [options]\n\n\
                     Options:\n  \
                       --compositor <command>  compositor to run (default: {DEFAULT_COMPOSITOR})\n  \
                       --app <command>         app to start; repeatable, replaces the defaults\n  \
                       --no-apps               start the compositor only\n  \
                       -h, --help              this message\n\n\
                     Default apps: {}",
                    env!("CARGO_PKG_VERSION"),
                    apps::DEFAULT_APPS.join(", ")
                );
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    Ok(Args { compositor, apps })
}

fn main() -> std::process::ExitCode {
    let args = match parse_args() {
        Ok(args) => args,
        Err(err) => {
            eprintln!("wlrix-session: {err}");
            return std::process::ExitCode::from(2);
        }
    };

    let environment = Environment::base();
    // SAFETY: startup, before any thread is spawned.
    unsafe { environment.export() };

    println!("wlrix-session: starting {}", args.compositor);
    let mut compositor = match compositor::start(&args.compositor, &environment) {
        Ok(compositor) => compositor,
        Err(err) => {
            eprintln!("wlrix-session: {err}");
            return std::process::ExitCode::FAILURE;
        }
    };

    // From here on a shutdown request has somewhere to go. Installed before the apps
    // start, so a signal arriving mid-startup still brings the compositor down rather
    // than orphaning it.
    signals::forward_to(compositor.child.id());

    for key in ["WAYLAND_DISPLAY", "DISPLAY"] {
        match compositor.environment.get(key) {
            Some(value) => println!("wlrix-session: {key}={value}"),
            None => println!("wlrix-session: {key} unset"),
        }
    }

    let started = apps::start_all(&args.apps, &compositor.environment);

    // The compositor is the session. Nothing else is worth waiting on: if an app dies,
    // the desktop carries on, but when the compositor goes there is no desktop left.
    let status = compositor.child.wait();
    apps::stop_all(started);

    match status {
        Ok(status) => {
            println!("wlrix-session: compositor exited ({status})");
            // A compositor that died because we passed a shutdown signal on to it did
            // what was asked, even though it exited by signal rather than cleanly.
            if status.success() || signals::shutdown_requested() {
                std::process::ExitCode::SUCCESS
            } else {
                std::process::ExitCode::FAILURE
            }
        }
        Err(err) => {
            eprintln!("wlrix-session: could not wait for the compositor: {err}");
            std::process::ExitCode::FAILURE
        }
    }
}
