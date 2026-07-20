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

#[macro_use]
mod log;

mod apps;
mod compositor;
mod config;
mod environment;
mod signals;

use config::AppSpec;
use environment::Environment;

/// The compositor to run, unless `--compositor` says otherwise.
const DEFAULT_COMPOSITOR: &str = "wlrix-compositor";

/// What the command line asked for. Everything is optional: anything not given here is
/// taken from the config file, and anything absent there falls back to a built-in
/// default. The command line wins because it is the more deliberate act.
#[derive(Default)]
struct Args {
    compositor: Option<String>,
    apps: Option<Vec<AppSpec>>,
}

fn parse_args() -> Result<Args, String> {
    let mut compositor = None;
    let mut apps: Option<Vec<AppSpec>> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--compositor" => {
                compositor = Some(
                    args.next()
                        .ok_or_else(|| "--compositor needs a command".to_string())?,
                );
            }
            // Repeatable. Giving any replaces the configured list rather than adding
            // to it, so `--app foo` means "just foo".
            "--app" => {
                let app = args
                    .next()
                    .ok_or_else(|| "--app needs a command".to_string())?;
                apps.get_or_insert_default().push(AppSpec::command(app));
            }
            "--no-apps" => {
                apps = Some(Vec::new());
            }
            "--help" | "-h" => {
                println!(
                    "wlrix-session {}\n\n\
                     Usage: wlrix-session [options]\n\n\
                     Options:\n  \
                       --compositor <command>  compositor to run (default: {DEFAULT_COMPOSITOR})\n  \
                       --app <command>         app to start; repeatable, replaces the configured list\n  \
                       --no-apps               start the compositor only\n  \
                       -h, --help              this message\n\n\
                     Config: $XDG_CONFIG_HOME/wlrix/session.toml, else /etc/wlrix/session.toml\n\
                     Default apps when unconfigured: {}",
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
            warn!("{err}");
            return std::process::ExitCode::from(2);
        }
    };

    // Before anything is reported, so nothing is lost from the record.
    let log_path = log::init();
    if let Some(path) = &log_path {
        info!("logging to {}", path.display());
    }

    let loaded = config::load();
    match &loaded.source {
        config::Source::File(path) => info!("config {}", path.display()),
        config::Source::Rejected(path) => {
            info!("ignoring {}; using the default session", path.display())
        }
        config::Source::None => info!("no config file; using defaults"),
    }

    let mut environment = Environment::base();
    // After the base, so a config file can override what the session assumes.
    for (key, value) in &loaded.config.env {
        environment.set(key, value);
    }
    // SAFETY: startup, before any thread is spawned.
    unsafe { environment.export() };

    let compositor_command = args
        .compositor
        .or(loaded.config.compositor)
        .unwrap_or_else(|| DEFAULT_COMPOSITOR.to_string());

    // An empty list in a config file means "no apps", so the defaults only apply when
    // nothing said anything at all.
    let app_list: Vec<AppSpec> = match (args.apps, loaded.source.is_configured()) {
        (Some(apps), _) => apps,
        (None, true) => loaded.config.apps,
        (None, false) => apps::DEFAULT_APPS
            .iter()
            .map(|app| AppSpec::command(*app))
            .collect(),
    };

    info!("starting {compositor_command}");
    let mut compositor = match compositor::start(&compositor_command, &environment) {
        Ok(compositor) => compositor,
        Err(err) => {
            warn!("{err}");
            return std::process::ExitCode::FAILURE;
        }
    };

    // From here on a shutdown request has somewhere to go. Installed before the apps
    // start, so a signal arriving mid-startup still brings the compositor down rather
    // than orphaning it.
    signals::forward_to(compositor.child.id());

    for key in ["WAYLAND_DISPLAY", "DISPLAY"] {
        match compositor.environment.get(key) {
            Some(value) => info!("{key}={value}"),
            None => info!("{key} unset"),
        }
    }

    let started = apps::start_all(&app_list, &compositor.environment);

    // The compositor is the session. Nothing else is worth waiting on: if an app dies,
    // the desktop carries on, but when the compositor goes there is no desktop left.
    let status = compositor.child.wait();
    apps::stop_all(started);

    match status {
        Ok(status) => {
            info!("compositor exited ({status})");
            // A compositor that died because we passed a shutdown signal on to it did
            // what was asked, even though it exited by signal rather than cleanly.
            if status.success() || signals::shutdown_requested() {
                std::process::ExitCode::SUCCESS
            } else {
                std::process::ExitCode::FAILURE
            }
        }
        Err(err) => {
            warn!("could not wait for the compositor: {err}");
            std::process::ExitCode::FAILURE
        }
    }
}
