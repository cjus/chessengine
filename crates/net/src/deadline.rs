//! A deadline for a connection's first message, kept at the one place every byte passes: the
//! socket's read.
//!
//! A client that connects and says nothing, or sends a byte at a time, would hold the one place
//! for ever. So until its first message is complete, each wait for bytes is limited to what is
//! left of **one deadline for the whole message**, not a fresh timeout per read (which a
//! trickle of bytes would keep renewing). That has to hold whatever sits above the socket: a
//! plain buffered reader reads once per call, but TLS may read several times to assemble one
//! record, so the limit is applied by the socket reader ([`Gate`]) itself.
//!
//! After the first message the deadline is cleared and reads wait as long as the client likes:
//! see the crate docs, "Deadlines".

use std::io::{self, Read};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// When the first message is due, or nothing once it has arrived. Cloning shares it.
#[derive(Clone, Debug)]
pub(crate) struct Deadline(Arc<Mutex<Option<Instant>>>);

impl Deadline {
    /// A deadline `within` from now.
    pub(crate) fn after(within: Duration) -> Deadline {
        Deadline(Arc::new(Mutex::new(Some(Instant::now() + within))))
    }

    /// Whether a deadline is still running.
    pub(crate) fn is_set(&self) -> bool {
        self.get().is_some()
    }

    /// Ends the deadline: the first message has arrived.
    pub(crate) fn clear(&self) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    fn get(&self) -> Option<Instant> {
        *self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// The read side of a socket, limited by a [`Deadline`] while it runs.
pub(crate) struct Gate {
    socket: TcpStream,
    deadline: Deadline,
}

impl Gate {
    pub(crate) fn new(socket: TcpStream, deadline: Deadline) -> Gate {
        Gate { socket, deadline }
    }
}

impl Read for Gate {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if let Some(due) = self.deadline.get() {
            let left = due.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(io::ErrorKind::TimedOut.into());
            }
            self.socket.set_read_timeout(Some(left))?;
        }
        self.socket.read(out)
    }
}
