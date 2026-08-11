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
//!
//! `SIGCHLD` is here for an unrelated reason: it is how the session hears that one of its
//! apps has exited, so it can reap it. See [`watch_children`].

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

/// The compositor to pass signals to; zero until it is running.
static COMPOSITOR_PID: AtomicI32 = AtomicI32::new(0);
/// Whether a signal has already been forwarded once.
static ASKED_ONCE: AtomicBool = AtomicBool::new(false);
/// The self-pipe `SIGCHLD` is reported on: the handler writes a byte, [`wait_for_child_exit`]
/// blocks reading one. `-1` until [`watch_children`] has run.
static CHILD_PIPE_READ: AtomicI32 = AtomicI32::new(-1);
static CHILD_PIPE_WRITE: AtomicI32 = AtomicI32::new(-1);

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

/// Start reporting child exits, so the session can reap its apps as they go.
///
/// A signal handler cannot do the reaping itself -- `waitpid` on a particular child is not
/// something to be doing in signal context, and it would race the compositor's own `wait` --
/// so this is the standard self-pipe: the handler writes one byte, and the session's loop
/// blocks reading it and does the real work outside signal context.
///
/// Failing to set this up is reported and otherwise survivable: the session still runs, it
/// just leaves a zombie behind for each app the user closes, which is where it started.
pub fn watch_children() {
    let mut fds = [-1; 2];
    // Close-on-exec on both ends: these are the session's business and no child of it has
    // any use for them.
    // SAFETY: `fds` is a two-element array, which is what `pipe2` writes.
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        warn!(
            "could not watch for app exits: {}",
            std::io::Error::last_os_error()
        );
        return;
    }
    // The *write* end only is non-blocking, so the handler can never stall the process on a
    // full pipe. The read end stays blocking: that is what puts the session to sleep.
    // SAFETY: `fds[1]` is the pipe end just created.
    unsafe { libc::fcntl(fds[1], libc::F_SETFL, libc::O_NONBLOCK) };

    CHILD_PIPE_READ.store(fds[0], Ordering::SeqCst);
    CHILD_PIPE_WRITE.store(fds[1], Ordering::SeqCst);
    // SAFETY: the handler does async-signal-safe work only -- an atomic load, a `write` to a
    // non-blocking fd, and saving/restoring `errno` around it.
    unsafe {
        libc::signal(
            libc::SIGCHLD,
            handle_child as *const () as libc::sighandler_t,
        )
    };
}

/// Block until a child has exited, or return at once if one already has.
///
/// Wakes spuriously as well: every child exit writes a byte, the compositor's included, and
/// several exits can coalesce into one. The caller must therefore treat this as "something may
/// have happened, go and look" rather than as a count -- which is what re-checking every child
/// on each wake-up amounts to.
pub fn wait_for_child_exit() {
    let fd = CHILD_PIPE_READ.load(Ordering::SeqCst);
    if fd < 0 {
        // No pipe: nothing will ever wake this, so do not block forever on it. Falling back
        // to a plain sleep keeps a session whose pipe could not be made merely inefficient
        // rather than unable to notice its compositor exiting.
        std::thread::sleep(std::time::Duration::from_secs(1));
        return;
    }
    let mut byte = [0u8; 1];
    // SAFETY: reading one byte into a one-byte buffer from an fd this module owns.
    unsafe { libc::read(fd, byte.as_mut_ptr() as *mut libc::c_void, 1) };
}

/// Runs in signal context, so it may only do async-signal-safe work.
extern "C" fn handle_child(_signal: libc::c_int) {
    let fd = CHILD_PIPE_WRITE.load(Ordering::SeqCst);
    if fd < 0 {
        return;
    }
    // A handler must leave `errno` as it found it: the interrupted code may be about to read
    // it, and `write` sets it on failure -- which this one is entitled to do, a full pipe
    // being a wake-up already pending rather than a problem.
    // SAFETY: `__errno_location` and `write` are both async-signal-safe.
    unsafe {
        let errno = libc::__errno_location();
        let saved = *errno;
        let byte = 0u8;
        libc::write(fd, std::ptr::from_ref(&byte).cast(), 1);
        *errno = saved;
    }
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
