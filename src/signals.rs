// SPDX-License-Identifier: GPL-3.0-or-later
//! Shutting the session down when something asks it to.
//!
//! A display manager stops a session by sending `SIGTERM`, and a person running it from
//! a terminal presses Ctrl+C. Either would otherwise kill `wlrix-session` alone and
//! leave the compositor holding the display with the apps still running -- on a TTY
//! that means a screen nobody owns and no way back.
//!
//! So the signal is not handled here so much as passed on: the compositor is told to
//! exit, its own teardown runs, and the session's normal path takes over -- `wait`
//! returns, the apps are stopped, and the exit status is the compositor's. One shutdown
//! path, whether the compositor quit on its own or was asked to.
//!
//! A second signal means the first did not take, so the compositor is killed outright.

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

/// The compositor to pass signals to; zero until it is running.
static COMPOSITOR_PID: AtomicI32 = AtomicI32::new(0);
/// Whether a signal has already been forwarded once.
static ASKED_ONCE: AtomicBool = AtomicBool::new(false);

/// Pass `SIGTERM` and `SIGINT` on to `pid` instead of dying and orphaning it.
pub fn forward_to(pid: u32) {
    COMPOSITOR_PID.store(pid as i32, Ordering::SeqCst);
    for signal in [libc::SIGTERM, libc::SIGINT] {
        // SAFETY: installing a handler that only does async-signal-safe work (an
        // atomic load and `kill`), which is what the restriction on signal handlers
        // is about.
        unsafe { libc::signal(signal, handle as *const () as libc::sighandler_t) };
    }
}

/// Whether the session asked the compositor to stop, rather than it exiting on its own.
///
/// A compositor killed by a signal we sent is an orderly shutdown, not a failure, and
/// the session's exit status should say so.
pub fn shutdown_requested() -> bool {
    ASKED_ONCE.load(Ordering::SeqCst)
}

/// Runs in signal context, so it may only do async-signal-safe work.
extern "C" fn handle(_signal: libc::c_int) {
    let pid = COMPOSITOR_PID.load(Ordering::SeqCst);
    if pid <= 0 {
        return;
    }
    // First ask, then insist. A compositor that has not gone after being asked is
    // wedged, and leaving a wedged compositor holding the display is the worse
    // outcome -- there is no way back to the TTY from it.
    let signal = if ASKED_ONCE.swap(true, Ordering::SeqCst) {
        libc::SIGKILL
    } else {
        libc::SIGTERM
    };
    // SAFETY: `kill` is async-signal-safe.
    unsafe { libc::kill(pid, signal) };
}
