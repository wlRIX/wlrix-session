// SPDX-License-Identifier: GPL-3.0-or-later
//! Undoing a low priority handed down by whatever started the session.
//!
//! Scheduling priority and I/O class are inherited, so the whole desktop runs at whatever its
//! launcher ran at. That is not always ordinary: CachyOS's ananicy-cpp files greetd under
//! "Launcher", nice 16 and the idle I/O class, and its session worker passes both on. A desktop
//! at nice 16 loses the CPU to any build, and in the idle I/O class it gets the disk only when
//! nothing else wants it -- the cursor stutters and a video stalls for as long as the build runs.
//!
//! So the session puts itself back at the defaults before it starts anything, and everything
//! it starts inherits those instead. Only a *lowered* priority is undone: a negative nice was
//! given on purpose, and is no hazard to hand down.

/// `ioprio_set`/`ioprio_get`'s "which": a single thread or process.
const IOPRIO_WHO_PROCESS: libc::c_int = 1;
/// The I/O class sits in the top bits of an I/O priority, above the level.
const IOPRIO_CLASS_SHIFT: libc::c_int = 13;
/// The class that is served only when the disk is otherwise idle.
const IOPRIO_CLASS_IDLE: libc::c_int = 3;
/// No class at all, which the kernel reads as best-effort at a level derived from the nice
/// value: what a process gets when nobody has set one.
const IOPRIO_NONE: libc::c_int = 0;

/// Put this thread back at nice 0 and the default I/O class, if it was started below them.
///
/// Per thread, as both settings are on Linux, so this has to run before the session spawns
/// any: threads and children made afterwards inherit what it leaves.
pub fn reset() {
    reset_nice();
    reset_io_class();
}

fn reset_nice() {
    // `getpriority` returns the nice value itself, so -1 is a valid answer and only `errno`
    // tells it from a failure.
    // SAFETY: `__errno_location` points at this thread's `errno`; `getpriority` has no
    // pointer arguments.
    let nice = unsafe {
        *libc::__errno_location() = 0;
        libc::getpriority(libc::PRIO_PROCESS, 0)
    };
    if std::io::Error::last_os_error().raw_os_error() != Some(0) || nice <= 0 {
        return;
    }
    // Raising priority back to 0 needs `RLIMIT_NICE` of 20 or more; a system that does not
    // grant it keeps the session where it was started, which is worth saying.
    // SAFETY: no pointer arguments.
    if unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, 0) } == 0 {
        info!("started at nice {nice}; reset to 0");
    } else {
        warn!(
            "started at nice {nice} and could not reset it: {}",
            std::io::Error::last_os_error()
        );
    }
}

fn reset_io_class() {
    // SAFETY: `ioprio_get` takes two integers and returns one.
    let ioprio = unsafe { libc::syscall(libc::SYS_ioprio_get, IOPRIO_WHO_PROCESS, 0) };
    if ioprio < 0 || (ioprio as libc::c_int) >> IOPRIO_CLASS_SHIFT != IOPRIO_CLASS_IDLE {
        return;
    }
    // Leaving the idle class for the default one needs no privilege.
    // SAFETY: `ioprio_set` takes three integers.
    if unsafe { libc::syscall(libc::SYS_ioprio_set, IOPRIO_WHO_PROCESS, 0, IOPRIO_NONE) } == 0 {
        info!("started in the idle I/O class; reset to the default");
    } else {
        warn!(
            "started in the idle I/O class and could not leave it: {}",
            std::io::Error::last_os_error()
        );
    }
}
