//! Limits on how much one address can ask of the listener before anyone is approved.
//!
//! A stranger costs the listener a thread and a handshake. Two rules keep one address from
//! spending them all, **before** it has proved anything:
//!
//! - **At most [`PER_ADDRESS`] connections at once** from one address are being greeted; the next
//!   is closed unanswered. Other addresses are unaffected, so one machine's flood doesn't keep
//!   another's approved client out. (The listener's overall cap, [`crate::MAX_GREETINGS`], remains
//!   for floods from many addresses.)
//! - **After [`FAILURES`] failed greetings in [`WINDOW`], an address is ignored for [`BAN`].** A
//!   failure is a handshake that fails or times out, or a stranger who isn't approved and
//!   doesn't pair. A client that pairs, or is approved, counts for nothing. This slows
//!   guessing and floods without a clock to set or a list to clear: it expires by itself.
//!
//! Both are by address, so people behind one shared address (a household's router, seen from
//! the internet) share one allowance. For a service meant for a home or office network, where
//! each machine has its own address, that is the cost of not asking anything of the person.
//!
//! The tables are bounded ([`TRACKED`] addresses); past that bound they forget the oldest, so a
//! flood from many addresses can't grow memory without end, at the price of forgetting a ban.

use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};

/// Connections from one address greeted at once, at most.
pub const PER_ADDRESS: usize = 4;

/// Failed greetings from one address, within [`WINDOW`], that earn a [`BAN`].
pub const FAILURES: u32 = 8;

/// The period failures are counted over.
pub const WINDOW: Duration = Duration::from_secs(10);

/// How long an address that failed too often is ignored.
pub const BAN: Duration = Duration::from_secs(30);

/// How many addresses the tables remember.
const TRACKED: usize = 1024;

/// What the listener remembers of the addresses that failed to be greeted.
#[derive(Debug, Default)]
pub(crate) struct Abuse {
    /// Failures counted since the window opened, for each address.
    failures: HashMap<IpAddr, (Instant, u32)>,
    /// Addresses ignored until the time given.
    banned: HashMap<IpAddr, Instant>,
}

impl Abuse {
    /// Whether a new connection from `ip` may be greeted, given how many from it are being
    /// greeted already.
    pub(crate) fn may_greet(&mut self, ip: IpAddr, in_flight: usize, now: Instant) -> bool {
        if in_flight >= PER_ADDRESS {
            return false;
        }
        match self.banned.get(&ip) {
            Some(until) if *until > now => false,
            Some(_) => {
                self.banned.remove(&ip);
                true
            }
            None => true,
        }
    }

    /// Notes that greeting a connection from `ip` failed.
    pub(crate) fn failed(&mut self, ip: IpAddr, now: Instant) {
        if self.failures.len() >= TRACKED && !self.failures.contains_key(&ip) {
            self.failures
                .retain(|_, (since, _)| now.duration_since(*since) < WINDOW);
            if self.failures.len() >= TRACKED {
                self.failures.clear();
            }
        }
        let entry = self.failures.entry(ip).or_insert((now, 0));
        if now.duration_since(entry.0) >= WINDOW {
            *entry = (now, 0);
        }
        entry.1 += 1;
        if entry.1 >= FAILURES {
            self.failures.remove(&ip);
            if self.banned.len() >= TRACKED {
                self.banned.retain(|_, until| *until > now);
                if self.banned.len() >= TRACKED {
                    self.banned.clear();
                }
            }
            self.banned.insert(ip, now + BAN);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(n: u8) -> IpAddr {
        IpAddr::from([10, 0, 0, n])
    }

    #[test]
    fn an_address_may_have_a_few_connections_greeted_at_once() {
        let mut abuse = Abuse::default();
        let now = Instant::now();
        assert!(abuse.may_greet(ip(1), PER_ADDRESS - 1, now));
        assert!(!abuse.may_greet(ip(1), PER_ADDRESS, now));
    }

    #[test]
    fn enough_failures_in_the_window_earn_a_ban_for_that_address_only() {
        let mut abuse = Abuse::default();
        let t0 = Instant::now();
        for _ in 0..FAILURES - 1 {
            abuse.failed(ip(1), t0);
        }
        assert!(abuse.may_greet(ip(1), 0, t0), "one short of a ban");
        abuse.failed(ip(1), t0);
        assert!(!abuse.may_greet(ip(1), 0, t0));
        assert!(!abuse.may_greet(ip(1), 0, t0 + BAN - Duration::from_secs(1)));
        assert!(
            abuse.may_greet(ip(2), 0, t0),
            "another address is unaffected"
        );
        assert!(
            abuse.may_greet(ip(1), 0, t0 + BAN),
            "the ban ends by itself"
        );
    }

    #[test]
    fn failures_spread_over_more_than_the_window_never_add_up() {
        let mut abuse = Abuse::default();
        let t0 = Instant::now();
        for i in 0..50 {
            abuse.failed(ip(1), t0 + WINDOW * i);
        }
        assert!(abuse.may_greet(ip(1), 0, t0 + WINDOW * 50));
    }

    #[test]
    fn the_tables_stay_bounded_under_a_flood_from_many_addresses() {
        let mut abuse = Abuse::default();
        let t0 = Instant::now();
        for n in 0..(TRACKED as u32 * 4) {
            let ip = IpAddr::from(n.to_be_bytes());
            for _ in 0..FAILURES {
                abuse.failed(ip, t0);
            }
        }
        assert!(abuse.failures.len() <= TRACKED);
        assert!(abuse.banned.len() <= TRACKED);
    }
}
