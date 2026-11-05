//! Who may connect: the approved clients, and the short window in which a new one may pair.
//!
//! Everything is a small text file in the data directory ([`crate::files::default_dir`]), so a
//! person can read it, and revoke a client by deleting its line:
//!
//! - `identity.pem`: this node's certificate and key ([`crate::Identity`]).
//! - `approved-clients.txt`: one line for each client that may connect, `fingerprint date name`.
//! - `pairing.txt`: present only while a pairing window is open (`cb2026 --pair`): the one-time
//!   code, when it expires, and how many wrong guesses are left.
//!
//! The listener (`cb2026 --serve`) and the pairing command (`cb2026 --pair`) are separate
//! processes that meet only in these files, so the listener reads the approved list for each
//! connection (a revoked client is refused at once, with no restart) and opens no way for one
//! process to command the other.
//!
//! ## Why a code, and why it can't be guessed
//!
//! A client the node has not seen asks to pair by sending the code the person was shown on the
//! node's screen. The code proves the person at the node's keyboard let this client in; the
//! fingerprint, which the person compares on the client's screen, proves the client is talking to
//! this node and not to someone in the middle. The code is eight digits and a window allows
//! [`PAIRING_ATTEMPTS`] wrong guesses before it closes, so the chance of guessing it is a few in a
//! hundred million for each window, and a window lasts [`PAIRING_MINUTES`] minutes at most.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::files;
use crate::identity::{Fingerprint, Identity};

/// How long a pairing window stays open, at most.
pub const PAIRING_MINUTES: u64 = 5;

/// How many wrong codes close a pairing window.
pub const PAIRING_ATTEMPTS: u32 = 3;

/// Digits in a pairing code.
const CODE_DIGITS: usize = 8;

/// The longest name kept for a client, in characters.
const NAME_CHARS: usize = 64;

/// A client that may connect.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Approved {
    /// The fingerprint of its certificate.
    pub fingerprint: Fingerprint,
    /// The day it was approved, as `YYYY-MM-DD` (empty if the line has none).
    pub date: String,
    /// The name it gave when pairing, to tell clients apart in the file.
    pub name: String,
}

/// An open pairing window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pairing {
    /// The one-time code the person gives the client, as digits.
    pub code: String,
    /// When the window closes.
    pub expires: SystemTime,
}

impl Pairing {
    /// The code in two groups of four, to read aloud and type.
    pub fn code_in_groups(&self) -> String {
        format!(
            "{} {}",
            &self.code[..CODE_DIGITS / 2],
            &self.code[CODE_DIGITS / 2..]
        )
    }
}

/// How a client's request to pair came out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paired {
    /// The code was right: the client is approved from now on, and the window is closed.
    Yes,
    /// No window is open, it has expired, or the code was wrong. Nothing says which.
    No,
}

/// The directory of a node's trust files, shared between the listener's threads.
#[derive(Debug)]
pub struct Store {
    dir: PathBuf,
    /// Held while a pairing attempt reads and rewrites the window, so two clients can't both
    /// spend the same last guess, or both win the same code.
    pairing: Mutex<()>,
}

impl Store {
    /// The trust files in `dir`, which need not exist yet.
    pub fn new(dir: impl Into<PathBuf>) -> Store {
        Store {
            dir: dir.into(),
            pairing: Mutex::new(()),
        }
    }

    /// The directory the files are in.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn identity_path(&self) -> PathBuf {
        self.dir.join("identity.pem")
    }

    fn approved_path(&self) -> PathBuf {
        self.dir.join("approved-clients.txt")
    }

    fn pairing_path(&self) -> PathBuf {
        self.dir.join("pairing.txt")
    }

    /// This node's identity, made on first use.
    pub fn identity(&self) -> io::Result<Identity> {
        Identity::load_or_create(&self.identity_path())
    }

    /// The approved clients, in the file's order. A line that isn't one (a comment, a blank, a
    /// fingerprint that doesn't read) is skipped, so a hand-edit can't lock everyone out.
    pub fn approved(&self) -> io::Result<Vec<Approved>> {
        let text = match fs::read_to_string(self.approved_path()) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e),
        };
        Ok(text.lines().filter_map(parse_line).collect())
    }

    /// Whether the client with this fingerprint may connect. A file that can't be read means no.
    pub fn is_approved(&self, fingerprint: &Fingerprint) -> bool {
        self.approved()
            .is_ok_and(|all| all.iter().any(|a| a.fingerprint == *fingerprint))
    }

    /// Approves a client from now on. A client already approved keeps its place (and its first
    /// date), with the new name.
    pub fn approve(&self, fingerprint: Fingerprint, name: &str, now: SystemTime) -> io::Result<()> {
        let mut all = self.approved()?;
        let name = clean_name(name);
        match all.iter_mut().find(|a| a.fingerprint == fingerprint) {
            Some(existing) => existing.name = name,
            None => all.push(Approved {
                fingerprint,
                date: date(now),
                name,
            }),
        }
        self.save(&all)
    }

    /// Removes a client, so it is refused from its next connection. Whether it was there.
    pub fn revoke(&self, fingerprint: &Fingerprint) -> io::Result<bool> {
        let mut all = self.approved()?;
        let before = all.len();
        all.retain(|a| a.fingerprint != *fingerprint);
        if all.len() == before {
            return Ok(false);
        }
        self.save(&all)?;
        Ok(true)
    }

    fn save(&self, all: &[Approved]) -> io::Result<()> {
        let mut text = String::from(
            "# cb2026 approved clients: a client whose fingerprint is listed here may connect.\n\
             # Each line is: fingerprint, the day it was approved, its name.\n\
             # Delete a line to revoke that client.\n",
        );
        for a in all {
            text.push_str(&format!("{} {} {}\n", a.fingerprint, a.date, a.name));
        }
        files::write_private(&self.approved_path(), text.as_bytes())
    }

    /// Opens a pairing window for [`PAIRING_MINUTES`] minutes, replacing any that was open, and
    /// returns its code.
    pub fn open_pairing(&self, now: SystemTime) -> io::Result<Pairing> {
        let _held = self.pairing.lock().unwrap_or_else(|e| e.into_inner());
        let pairing = Pairing {
            code: random_code()?,
            expires: now + Duration::from_secs(PAIRING_MINUTES * 60),
        };
        self.write_window(&pairing, PAIRING_ATTEMPTS)?;
        Ok(pairing)
    }

    /// Whether a pairing window is open now.
    pub fn pairing_is_open(&self, now: SystemTime) -> bool {
        self.read_window().is_some_and(|(p, _)| p.expires > now)
    }

    /// Closes the pairing window, if one is open.
    pub fn close_pairing(&self) {
        let _held = self.pairing.lock().unwrap_or_else(|e| e.into_inner());
        let _ = fs::remove_file(self.pairing_path());
    }

    /// A client asks to pair: approves it if a window is open and `code` is its code. A wrong
    /// code spends one of the window's guesses, and the last one closes it. Success closes the
    /// window too, so one code lets in one client.
    pub fn try_pair(
        &self,
        code: &str,
        fingerprint: Fingerprint,
        name: &str,
        now: SystemTime,
    ) -> io::Result<Paired> {
        let _held = self.pairing.lock().unwrap_or_else(|e| e.into_inner());
        let Some((window, attempts)) = self.read_window() else {
            return Ok(Paired::No);
        };
        if window.expires <= now {
            let _ = fs::remove_file(self.pairing_path());
            return Ok(Paired::No);
        }
        let given: String = code.chars().filter(|c| !matches!(c, ' ' | '-')).collect();
        if same(given.as_bytes(), window.code.as_bytes()) {
            // Approve before closing the window: a failure to save leaves the code to try again.
            self.approve(fingerprint, name, now)?;
            let _ = fs::remove_file(self.pairing_path());
            return Ok(Paired::Yes);
        }
        if attempts <= 1 {
            let _ = fs::remove_file(self.pairing_path());
        } else {
            self.write_window(&window, attempts - 1)?;
        }
        Ok(Paired::No)
    }

    fn write_window(&self, window: &Pairing, attempts: u32) -> io::Result<()> {
        let expires = window
            .expires
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let text = format!(
            "code={}\nexpires={expires}\nattempts={attempts}\n",
            window.code
        );
        files::write_private(&self.pairing_path(), text.as_bytes())
    }

    /// The open window and its guesses left, or `None` if there is none or it is damaged.
    fn read_window(&self) -> Option<(Pairing, u32)> {
        let text = fs::read_to_string(self.pairing_path()).ok()?;
        let field = |key: &str| {
            text.lines()
                .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
        };
        let code = field("code")?.to_string();
        if code.len() != CODE_DIGITS || !code.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let expires = UNIX_EPOCH + Duration::from_secs(field("expires")?.parse().ok()?);
        let attempts = field("attempts")?.parse().ok()?;
        Some((Pairing { code, expires }, attempts))
    }
}

/// Whether two byte strings are equal, taking as long when they differ early as when they differ
/// late, so a client can't learn a code a digit at a time from how fast it is refused.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0, |diff, (x, y)| diff | (x ^ y)) == 0
}

/// A name safe to keep in the approved file: one line, no control characters, not too long.
fn clean_name(name: &str) -> String {
    let name: String = name
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(NAME_CHARS)
        .collect();
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        "unnamed client".to_string()
    } else {
        name
    }
}

fn parse_line(line: &str) -> Option<Approved> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let (first, rest) = line.split_once(char::is_whitespace)?;
    let fingerprint = first.parse().ok()?;
    let rest = rest.trim();
    let (date, name) = match rest.split_once(char::is_whitespace) {
        Some((d, n)) if is_date(d) => (d.to_string(), n.trim()),
        None if is_date(rest) => (rest.to_string(), ""),
        _ => (String::new(), rest),
    };
    Some(Approved {
        fingerprint,
        date,
        name: name.to_string(),
    })
}

fn is_date(text: &str) -> bool {
    let b = text.as_bytes();
    b.len() == 10
        && b.iter().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 {
                *c == b'-'
            } else {
                c.is_ascii_digit()
            }
        })
}

/// Eight random digits from the system's secure random source.
fn random_code() -> io::Result<String> {
    let mut bytes = [0u8; 8];
    rustls::crypto::ring::default_provider()
        .secure_random
        .fill(&mut bytes)
        .map_err(|_| io::Error::other("the system has no secure random numbers"))?;
    // 2^64 is far above 10^8, so the remainder's bias is below one in 10^11.
    let number = u64::from_le_bytes(bytes) % 10u64.pow(CODE_DIGITS as u32);
    Ok(format!("{number:0width$}", width = CODE_DIGITS))
}

/// A day as `YYYY-MM-DD` (UTC), from the civil-date arithmetic of Howard Hinnant's
/// `civil_from_days` (public domain).
fn date(time: SystemTime) -> String {
    let days = time
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| (d.as_secs() / 86_400) as i64);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_part + 2) / 5 + 1;
    let month = if month_part < 10 {
        month_part + 3
    } else {
        month_part - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(name: &str) -> Store {
        Store::new(files::temp_dir(name))
    }

    fn fingerprint(n: u8) -> Fingerprint {
        Fingerprint::of(&rustls::pki_types::CertificateDer::from(vec![n]))
    }

    fn at(seconds: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(seconds)
    }

    #[test]
    fn dates_come_out_as_calendar_days() {
        assert_eq!(date(UNIX_EPOCH), "1970-01-01");
        assert_eq!(date(at(951_782_400)), "2000-02-29");
        assert_eq!(date(at(1_793_836_800)), "2026-11-05");
        assert_eq!(date(at(1_835_308_800 + 86_400)), "2028-02-29");
    }

    #[test]
    fn nobody_is_approved_until_approved_and_revoking_removes_them() {
        let store = store("approve");
        let a = fingerprint(1);
        assert!(!store.is_approved(&a));
        store.approve(a, "iMac", at(1_793_836_800)).unwrap();
        store
            .approve(fingerprint(2), "laptop", at(1_793_836_800))
            .unwrap();
        assert!(store.is_approved(&a));
        assert!(store.is_approved(&fingerprint(2)));
        assert!(!store.is_approved(&fingerprint(3)));
        assert_eq!(store.approved().unwrap()[0].date, "2026-11-05");
        assert!(store.revoke(&a).unwrap());
        assert!(!store.is_approved(&a));
        assert!(store.is_approved(&fingerprint(2)));
        assert!(!store.revoke(&a).unwrap(), "nothing left to revoke");
    }

    #[test]
    fn approving_again_renames_and_keeps_the_first_date() {
        let store = store("reapprove");
        store.approve(fingerprint(1), "old", at(0)).unwrap();
        store
            .approve(fingerprint(1), "new", at(1_793_836_800))
            .unwrap();
        let all = store.approved().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(
            (all[0].name.as_str(), all[0].date.as_str()),
            ("new", "1970-01-01")
        );
    }

    #[test]
    fn a_hand_edited_file_still_reads() {
        let store = store("hand-edit");
        let (a, b) = (fingerprint(1), fingerprint(2));
        fs::write(
            store.approved_path(),
            format!(
                "# comment\n\n{} Carlos's iMac\nnot-a-fingerprint x\n  {}\t2026-11-05\tlaptop two  \n",
                a.to_string().to_lowercase(),
                b
            ),
        )
        .unwrap();
        let all = store.approved().unwrap();
        assert_eq!(all.len(), 2, "{all:?}");
        assert_eq!(
            (all[0].fingerprint, all[0].name.as_str()),
            (a, "Carlos's iMac")
        );
        assert_eq!(
            (all[1].date.as_str(), all[1].name.as_str()),
            ("2026-11-05", "laptop two")
        );
    }

    #[test]
    fn a_name_cannot_break_the_file() {
        let store = store("names");
        store
            .approve(fingerprint(1), "evil\n0000 injected\r\nname", at(0))
            .unwrap();
        store.approve(fingerprint(2), "   ", at(0)).unwrap();
        store
            .approve(fingerprint(3), &"x".repeat(500), at(0))
            .unwrap();
        let all = store.approved().unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].name, "evil 0000 injected name");
        assert_eq!(all[1].name, "unnamed client");
        assert_eq!(all[2].name.len(), NAME_CHARS);
    }

    #[test]
    fn the_right_code_pairs_once() {
        let store = store("pair-once");
        let now = at(1_000);
        let window = store.open_pairing(now).unwrap();
        assert_eq!(window.code.len(), CODE_DIGITS);
        assert!(store.pairing_is_open(now));
        let spaced = window.code_in_groups();
        assert_eq!(
            store
                .try_pair(&spaced, fingerprint(1), "iMac", now)
                .unwrap(),
            Paired::Yes
        );
        assert!(store.is_approved(&fingerprint(1)));
        // The window closed with the first success: the same code lets in nobody else.
        assert!(!store.pairing_is_open(now));
        assert_eq!(
            store
                .try_pair(&window.code, fingerprint(2), "other", now)
                .unwrap(),
            Paired::No
        );
        assert!(!store.is_approved(&fingerprint(2)));
    }

    #[test]
    fn three_wrong_codes_close_the_window_even_for_the_right_one() {
        let store = store("pair-guesses");
        let now = at(1_000);
        let window = store.open_pairing(now).unwrap();
        let wrong = if window.code == "00000000" {
            "11111111"
        } else {
            "00000000"
        };
        for _ in 0..PAIRING_ATTEMPTS {
            assert_eq!(
                store.try_pair(wrong, fingerprint(9), "x", now).unwrap(),
                Paired::No
            );
        }
        assert!(!store.pairing_is_open(now));
        assert_eq!(
            store
                .try_pair(&window.code, fingerprint(1), "iMac", now)
                .unwrap(),
            Paired::No
        );
        assert!(!store.is_approved(&fingerprint(1)));
    }

    #[test]
    fn two_wrong_codes_leave_the_window_open() {
        let store = store("pair-typos");
        let now = at(1_000);
        let window = store.open_pairing(now).unwrap();
        let wrong = if window.code == "00000000" {
            "11111111"
        } else {
            "00000000"
        };
        for _ in 0..PAIRING_ATTEMPTS - 1 {
            store.try_pair(wrong, fingerprint(9), "x", now).unwrap();
        }
        assert_eq!(
            store
                .try_pair(&window.code, fingerprint(1), "iMac", now)
                .unwrap(),
            Paired::Yes
        );
    }

    #[test]
    fn an_expired_window_pairs_nobody_and_is_removed() {
        let store = store("pair-expiry");
        let window = store.open_pairing(at(1_000)).unwrap();
        let late = at(1_000 + PAIRING_MINUTES * 60);
        assert!(!store.pairing_is_open(late));
        assert_eq!(
            store
                .try_pair(&window.code, fingerprint(1), "iMac", late)
                .unwrap(),
            Paired::No
        );
        assert!(!store.pairing_path().exists());
    }

    #[test]
    fn with_no_window_nothing_pairs() {
        let store = store("pair-none");
        assert_eq!(
            store
                .try_pair("12345678", fingerprint(1), "iMac", at(0))
                .unwrap(),
            Paired::No
        );
        store.close_pairing();
    }

    #[test]
    fn a_damaged_window_is_no_window() {
        let store = store("pair-damaged");
        fs::write(store.pairing_path(), "code=12\nexpires=x\n").unwrap();
        assert!(!store.pairing_is_open(at(0)));
        assert_eq!(
            store.try_pair("12", fingerprint(1), "iMac", at(0)).unwrap(),
            Paired::No
        );
    }

    #[test]
    fn codes_differ_and_are_all_digits() {
        let codes: std::collections::HashSet<_> = (0..50).map(|_| random_code().unwrap()).collect();
        assert!(codes.len() > 45, "codes repeat: {}", codes.len());
        assert!(
            codes
                .iter()
                .all(|c| c.len() == 8 && c.bytes().all(|b| b.is_ascii_digit()))
        );
    }

    #[test]
    fn comparison_takes_the_whole_string() {
        assert!(same(b"12345678", b"12345678"));
        assert!(!same(b"12345678", b"12345679"));
        assert!(!same(b"1234567", b"12345678"));
    }
}
