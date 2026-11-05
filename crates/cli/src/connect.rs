//! `cb2026 --connect`: a command-line client of a cb2026 node on another computer.
//!
//! It exists so network play can be tried, and a client such as Solrac's can be checked against
//! a real one, before any window has a "Network" panel. It follows the steps in
//! [`cb2026_net::client`]: find the node, show the person its fingerprint to compare with the
//! one on the node's screen, send the pairing code only once they say it matches, remember the
//! node, then talk the protocol: here, a short test search.
//!
//! This client keeps two files in its data directory (the same default as `--serve`, which is
//! why a computer can be both): `client-identity.pem`, who it is, and `engines.txt`, the
//! fingerprints of the nodes it has paired with, one per line (`fingerprint name`). Deleting a
//! line makes it ask again.

use std::io::{self, BufRead, Write};
use std::net::{SocketAddr, ToSocketAddrs};
use std::time::Duration;

use cb2026_net::client::{self, Connection, Found};
use cb2026_net::{Fingerprint, Identity, Store};
use serde_json::{Value, json};

/// How long a search for nodes on the local network listens.
const DISCOVER_WAIT: Duration = Duration::from_secs(3);

/// How long the test search thinks, in milliseconds.
const TEST_SEARCH_MS: u64 = 1000;

/// The port a typed address uses when it has none: the one `--serve` and discovery use.
const DEFAULT_PORT: u16 = cb2026_net::DEFAULT_PORT;

const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

/// Connects to `target` (or to a node it finds), pairs if it must, and runs the test. Whether
/// the node answered the search.
pub fn run(target: Option<String>, store: &Store) -> Result<bool, String> {
    let identity = Identity::load_or_create(&store.dir().join("client-identity.pem"))
        .map_err(|e| format!("this client's identity: {e}"))?;
    let engines = store.dir().join("engines.txt");
    let mut input = io::stdin().lock();

    let (address, announced) = match target {
        Some(text) => (resolve(&text)?, None),
        None => match choose(&mut input)? {
            Some(found) => (found.address, Some(found.announcement)),
            None => return Ok(false),
        },
    };
    println!("Connecting to {address}...");
    let mut connection = client::connect(address, &identity, None)
        .map_err(|e| format!("couldn't connect to {address}: {e}"))?;
    let shown = connection.server_fingerprint();
    if let Some(said) = &announced
        && said.fingerprint != shown
    {
        return Err(format!(
            "{address} answered the search with the fingerprint {} but now shows {shown}. \
             Something else may be answering there; nothing was sent.",
            said.fingerprint
        ));
    }

    let label = announced
        .as_ref()
        .map_or_else(|| address.to_string(), |a| a.name.clone());
    let known = load_engines(&engines)
        .into_iter()
        .find(|(f, _)| *f == shown);
    if let Some((_, name)) = &known {
        println!("This is {name}, which this client has paired with before.");
    } else {
        connection = pair(connection, address, &identity, &engines, &label, &mut input)?
            .ok_or_else(|| "not paired".to_string())?;
    }

    let result = test(&mut connection);
    if result.is_err() && known.is_some() {
        eprintln!(
            "The node didn't answer. If it was reset or this client's approval was removed, \
             delete its line from {} and run --connect again to pair anew.",
            engines.display()
        );
    }
    result
}

/// Looks for nodes and, if there is more than one, asks which. `None` if there are none.
fn choose(input: &mut impl BufRead) -> Result<Option<Found>, String> {
    println!("Looking for cb2026 nodes on the local network...");
    let mut found = client::discover(DISCOVER_WAIT).map_err(|e| format!("the search: {e}"))?;
    match found.len() {
        0 => {
            println!(
                "None answered. On the node's computer, run `cb2026 --serve 0.0.0.0:4026 \
                 --discover`, or give its address here: `cb2026 --connect ADDRESS[:PORT]`."
            );
            Ok(None)
        }
        1 => {
            let one = found.remove(0);
            println!("Found \"{}\" at {}.", one.announcement.name, one.address);
            Ok(Some(one))
        }
        n => {
            println!("Found {n} nodes:");
            for (i, f) in found.iter().enumerate() {
                println!("  {}. \"{}\" at {}", i + 1, f.announcement.name, f.address);
            }
            let answer = ask(input, &format!("Which one (1-{n})? "))?;
            match answer.trim().parse::<usize>() {
                Ok(k) if (1..=n).contains(&k) => Ok(Some(found.remove(k - 1))),
                _ => Err("that isn't one of them".to_string()),
            }
        }
    }
}

/// Pairs with the node on `connection`, after the person confirms its fingerprint, and returns
/// a connection to it that has been approved. `None` if the person declined.
fn pair(
    mut connection: Connection,
    address: SocketAddr,
    identity: &Identity,
    engines: &std::path::Path,
    label: &str,
    input: &mut impl BufRead,
) -> Result<Option<Connection>, String> {
    let shown = connection.server_fingerprint();
    println!("\nThis client hasn't paired with this node. Its fingerprint is:\n");
    for line in shown.in_lines().lines() {
        println!("     {line}");
    }
    println!("\nOn the node's computer, run `cb2026 --pair`. It shows a fingerprint and a code.");
    let answer = ask(
        input,
        "Is the fingerprint the same as the one on that screen? (yes/no) ",
    )?;
    if !matches!(answer.trim().to_lowercase().as_str(), "yes" | "y") {
        println!("Not paired. Nothing was sent to the node.");
        return Ok(None);
    }
    let code: String = ask(input, "Type the code from that screen: ")?
        .chars()
        .filter(char::is_ascii_digit)
        .collect();
    let me = client_name();
    let approved = connection
        .pair(&code, &me)
        .map_err(|e| format!("pairing: {e}"))?;
    if !approved {
        return Err(
            "the node didn't accept the code (wrong, or its window closed: it lasts five \
             minutes and three tries). Run `cb2026 --pair` there again for a new one."
                .to_string(),
        );
    }
    remember(engines, shown, label).map_err(|e| format!("keeping {}: {e}", engines.display()))?;
    println!("Paired. From now on this client connects without a code.\n");
    // The node closes the connection after pairing; this one is pinned to what was confirmed.
    client::connect(address, identity, Some(shown))
        .map(Some)
        .map_err(|e| format!("reconnecting to {address}: {e}"))
}

/// Runs the protocol: `initialize`, then one short search from the starting position.
fn test(connection: &mut Connection) -> Result<bool, String> {
    let init = request(
        connection,
        1,
        "initialize",
        json!({"protocol": "1.0", "client": {"name": "cb2026 --connect", "version": cb2026_engine::build_version().to_string()}}),
    )?;
    let node = &init["node"];
    println!(
        "Connected to {} {}, speaking protocol {}.",
        node["name"].as_str().unwrap_or("a node"),
        node["version"].as_str().unwrap_or(""),
        init["protocol"].as_str().unwrap_or("?")
    );
    if let Some(r) = init.get("resources") {
        println!(
            "It searches with {} thread(s) and a {} MB hash table.",
            r["threads"], r["hashMb"]
        );
    }
    println!("Searching the starting position for {TEST_SEARCH_MS} ms...");
    let result = request(
        connection,
        2,
        "engine/search",
        json!({"position": {"fen": START_FEN}, "limits": {"timeMs": TEST_SEARCH_MS}}),
    )?;
    let Some(best) = result["bestMove"].as_str() else {
        return Err("the search answered without a move".to_string());
    };
    println!(
        "Best move {best}, score {}, depth {}, {} nodes.",
        result["score"], result["depth"], result["nodes"]
    );
    // A polite goodbye; the node may already be gone.
    let _ = request(connection, 3, "shutdown", Value::Null);
    let _ = connection.send_line(r#"{"jsonrpc":"2.0","method":"exit"}"#);
    println!("The connection works.");
    Ok(true)
}

/// Sends one request and returns its result, skipping progress notifications.
fn request(
    connection: &mut Connection,
    id: u64,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    let mut message = json!({"jsonrpc": "2.0", "id": id, "method": method});
    if !params.is_null() {
        message["params"] = params;
    }
    connection
        .send_line(&message.to_string())
        .map_err(|e| format!("sending {method}: {e}"))?;
    loop {
        let line = connection
            .read_line()
            .map_err(|e| format!("waiting for {method}: {e}"))?
            .ok_or_else(|| format!("the node closed the connection before answering {method}"))?;
        let Ok(answer) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if answer["id"] != json!(id) {
            continue;
        }
        if let Some(error) = answer.get("error") {
            return Err(format!(
                "{method}: {} (code {})",
                error["message"].as_str().unwrap_or("an error"),
                error["code"]
            ));
        }
        return Ok(answer["result"].clone());
    }
}

/// A typed address as a socket address: `HOST`, `HOST:PORT` or `IP:PORT`, the port defaulting
/// to the standard one.
fn resolve(text: &str) -> Result<SocketAddr, String> {
    let attempt = text
        .to_socket_addrs()
        .or_else(|_| (text, DEFAULT_PORT).to_socket_addrs());
    attempt
        .ok()
        .and_then(|mut found| found.next())
        .ok_or_else(|| format!("can't find an address for '{text}' (use HOST or HOST:PORT)"))
}

/// What this client calls itself in the node's list of approved clients.
fn client_name() -> String {
    ["HOSTNAME", "COMPUTERNAME", "HOST"]
        .iter()
        .find_map(|v| std::env::var(v).ok().filter(|n| !n.trim().is_empty()))
        .map(|n| n.chars().filter(|c| !c.is_control()).take(40).collect())
        .unwrap_or_else(|| "cb2026 --connect".to_string())
}

/// Prints `question` and reads one line of the answer.
fn ask(input: &mut impl BufRead, question: &str) -> Result<String, String> {
    print!("{question}");
    let _ = io::stdout().flush();
    let mut line = String::new();
    match input.read_line(&mut line) {
        Ok(0) => Err("no answer was given (input ended)".to_string()),
        Ok(_) => Ok(line),
        Err(e) => Err(e.to_string()),
    }
}

/// The nodes in `engines.txt`: a line each, `fingerprint name`. Lines that don't read are skipped.
fn load_engines(path: &std::path::Path) -> Vec<(Fingerprint, String)> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let (fingerprint, name) = line.split_once(' ').unwrap_or((line, ""));
            Some((fingerprint.parse().ok()?, name.trim().to_string()))
        })
        .collect()
}

/// Adds a node to `engines.txt`.
fn remember(path: &std::path::Path, fingerprint: Fingerprint, name: &str) -> io::Result<()> {
    let name: String = name.chars().filter(|c| !c.is_control()).collect();
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{fingerprint} {name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_without_a_port_gets_the_standard_one() {
        assert_eq!(resolve("127.0.0.1").unwrap().port(), DEFAULT_PORT);
        assert_eq!(resolve("127.0.0.1:5000").unwrap().port(), 5000);
        assert!(resolve("").is_err());
    }
}
