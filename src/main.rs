// SPDX-License-Identifier: GPL-3.0-or-later
//! wlRIX session manager — scaffold entry point.
//!
//! Sets up the environment, launches `wlrix-compositor`, then the core desktop
//! apps (toolchest, desks), and supervises them. For now it only lists the
//! components it will start once they exist.

/// Components the session will spawn, in order, once implemented.
const SESSION_PROGRAM: &[&str] = &["wlrix-compositor", "wlrix-toolchest", "wlrix-desks"];

fn main() {
    println!(
        "wlrix-session {} — scaffold. Will launch: {}",
        env!("CARGO_PKG_VERSION"),
        SESSION_PROGRAM.join(", ")
    );
}
