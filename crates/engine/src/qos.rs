//! Thread quality of service: asking macOS to keep the search on the performance cores.
//!
//! Apple Silicon has two kinds of cores: fast **performance** (P) cores and slower, frugal
//! **efficiency** (E) cores (an M1 Max has 8 and 2, an M3 Max 10 or 12 and 4). macOS doesn't let
//! a program pin a thread to a core. It decides where each thread runs, and how it competes with
//! other threads for a core, from the thread's **quality of service** (QoS) class:
//!
//! | Class | Meant for | Value |
//! | --- | --- | --- |
//! | user-interactive | drawing the screen, reacting to input | `0x21` |
//! | user-initiated | work a person started and is waiting for | `0x19` |
//! | default | anything that didn't ask | `0x15` |
//! | utility | long jobs with a progress bar | `0x11` |
//! | background | work nobody is waiting for (backups, indexing) | `0x09` |
//!
//! Higher classes get a core first, and the system is more likely to run the lowest on the
//! E-cores. A chess engine thinking on a person's clock is the textbook case of
//! "user-initiated".
//!
//! **What cb2026 does.** Every search thread asks for user-initiated at the start of a search
//! ([`SearchQos::raise`]), unless it already has a higher class, and the calling thread gets its
//! old class back when the search ends. The switch is [`crate::Engine::thread_qos`] (shell
//! `set qos`), on by default. It does nothing on other systems. Like the other switches, it
//! isn't a UCI option, so GUI option lists stay short (`docs/PRD.md`, "UCI").
//!
//! **Why it's needed.** A thread made with `std::thread::spawn` starts at *default*, whatever
//! class the thread that made it has (measured on macOS 26). UCI and XBoard search on such a
//! thread, and so do Lazy SMP's helper threads (chapter 17). So without this, every search thread
//! under a GUI competes at the same level as any other program's ordinary work.
//!
//! ## Pitfalls
//!
//! - **Never lower a class.** A program started from a terminal has a user-interactive main
//!   thread, and the shell searches on it. Asking for user-initiated there would demote it, so
//!   a thread already above user-initiated is left alone.
//! - **User-interactive is not for computing.** Apple asks for it to be kept to short work that
//!   keeps the screen responsive; a search running for minutes is user-initiated.
//! - **The process can be clamped.** macOS can hold a whole process down (`taskpolicy -b`, or a
//!   GUI launching its engines as background work). A thread's request doesn't lift that.
//! - **The request can be capped.** On the M1 Max, from Claude Code's shell, threads asking
//!   for user-initiated or user-interactive still got the scheduler priority of default (31 in
//!   `ps -M`), while utility and background dropped to 20 and 4. There the switch changed no
//!   measured speed. Whether a terminal window or a GUI allows more is to be checked.
//! - **Opted-out threads.** A thread whose priority was set with `pthread_setschedparam` has left
//!   the QoS system, and the request fails (`EPERM`). The search runs anyway; it just doesn't get
//!   the class.
//!
//! See Apple's Energy Efficiency Guide for Mac Apps, [Prioritize Work at the Task Level](https://developer.apple.com/library/archive/documentation/Performance/Conceptual/power_efficiency_guidelines_osx/PrioritizeWorkAtTheTaskLevel.html),
//! and [Tuning your code's performance for Apple silicon](https://developer.apple.com/documentation/apple-silicon/tuning-your-code-s-performance-for-apple-silicon),
//! and the declarations in the macOS SDK's `<sys/qos.h>` and `<pthread/qos.h>`.
//!
//! ## `unsafe`
//!
//! This is one of the crate's marked `unsafe` modules: declaring functions written in C is
//! `unsafe`, since Rust can't check that the declarations match them.

#![allow(unsafe_code)]

/// The class a search thread asks for: `QOS_CLASS_USER_INITIATED`.
pub const USER_INITIATED: u32 = 0x19;

/// `QOS_CLASS_UNSPECIFIED`: a thread that has no class, or has left the QoS system. It can't be
/// asked for.
const UNSPECIFIED: u32 = 0x00;

/// Raises the current thread to [`USER_INITIATED`] for as long as it lives, and puts the old
/// class back when dropped. Keep it in a variable (`let _qos = ...`) for the length of the
/// search: `let _ = ...` would drop it at once.
#[derive(Debug)]
#[must_use = "the thread goes back to its old class as soon as this is dropped"]
pub struct SearchQos {
    /// The class to go back to, if this raised the thread.
    previous: Option<u32>,
}

impl SearchQos {
    // ANCHOR: raise
    /// Asks for [`USER_INITIATED`] if `on` and the thread's class is lower. Does nothing on
    /// systems without QoS classes.
    pub fn raise(on: bool) -> SearchQos {
        let mut previous = None;
        if on
            && let Some(class) = platform::current()
            && class < USER_INITIATED
            && platform::request(USER_INITIATED)
        {
            previous = Some(class);
        }
        SearchQos { previous }
    }
    // ANCHOR_END: raise

    /// Whether this raised the thread's class.
    pub fn raised(&self) -> bool {
        self.previous.is_some()
    }
}

impl Drop for SearchQos {
    fn drop(&mut self) {
        // A thread that had no class keeps user-initiated: "unspecified" can't be asked for.
        if let Some(class) = self.previous
            && class != UNSPECIFIED
        {
            platform::request(class);
        }
    }
}

/// The current thread's QoS class, if the system has them.
pub fn current_class() -> Option<u32> {
    platform::current()
}

#[cfg(target_os = "macos")]
mod platform {
    // SAFETY: these match the declarations in the macOS SDK: `qos_class_t
    // qos_class_self(void)` in <sys/qos.h>, and `int pthread_set_qos_class_self_np(qos_class_t,
    // int)` in <pthread/qos.h>, where `qos_class_t` is an `unsigned int` enum. Both are safe to
    // call with any arguments: they act only on the calling thread, and the setter checks its
    // arguments and returns `EINVAL` for bad ones. So they can be declared `safe`.
    unsafe extern "C" {
        safe fn qos_class_self() -> u32;
        safe fn pthread_set_qos_class_self_np(qos_class: u32, relative_priority: i32) -> i32;
    }

    /// The current thread's class.
    pub(super) fn current() -> Option<u32> {
        Some(qos_class_self())
    }

    /// Asks for `class` at the top of its range (relative priority 0); whether it worked.
    pub(super) fn request(class: u32) -> bool {
        pthread_set_qos_class_self_np(class, 0) == 0
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    /// Other systems have no QoS classes.
    pub(super) fn current() -> Option<u32> {
        None
    }

    /// Never called: there is no class to raise.
    pub(super) fn request(_class: u32) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_changes_nothing() {
        std::thread::spawn(|| {
            let before = current_class();
            let qos = SearchQos::raise(false);
            assert!(!qos.raised());
            assert_eq!(current_class(), before);
        })
        .join()
        .unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_spawned_thread_is_raised_and_put_back() {
        std::thread::spawn(|| {
            // A new thread starts at "default" (0x15).
            let before = current_class().unwrap();
            assert!(before < USER_INITIATED);
            let qos = SearchQos::raise(true);
            assert!(qos.raised());
            assert_eq!(current_class(), Some(USER_INITIATED));
            drop(qos);
            assert_eq!(current_class(), Some(before));
        })
        .join()
        .unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_higher_class_is_left_alone() {
        std::thread::spawn(|| {
            const USER_INTERACTIVE: u32 = 0x21;
            assert!(platform::request(USER_INTERACTIVE));
            let qos = SearchQos::raise(true);
            assert!(!qos.raised());
            assert_eq!(current_class(), Some(USER_INTERACTIVE));
        })
        .join()
        .unwrap();
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn other_systems_have_no_classes() {
        assert_eq!(current_class(), None);
        assert!(!SearchQos::raise(true).raised());
    }
}
