# Network play: a design for the opt-in listener (#209)

**Status: steps 1–6 built, 7 later.** The listener, TLS and pairing exist in a build with
`--features net`, which is off by default; an ordinary run opens no port. Carlos answered the five
decisions on 2026-11-05 (cjus/chessengine#255; [Decisions](#decisions)): pairing with both sides
pinning each other, one client at a time, discovery by UDP broadcast, limits set on the engine's
machine, and a separate `crates/net`. [How it would be built](#how-it-would-be-built) says what
each step did. Written 2026-11-05; steps 3 to 6 added the same day.

## What this is for

Run cb2026 on one computer and use it from a client on another, so a person can put the
processing power of the machines on their network behind their GUI. The client is Solrac Chess AI
(or any program that speaks the Chess Node Protocol); Solrac's side is its own work. This
document is the engine's half: **cb2026 listens, over CNP, on a port the user chose, and only
for clients they have approved.**

Out of scope: several machines cooperating on one search, a machine-level supervisor that runs a
pool of engines, and anything that needs a hosted service. Those are Solrac's distributed-search
work ("stage 2"); [Compatibility with stage 2](#compatibility-with-stage-2) says what this design
does so that work stays easy.

## What is settled

From #209, and not up for debate here:

- The protocol is **CNP**, one JSON-RPC message per line, over a stream. No second chess
  command language.
- **Opt-in.** An ordinary run opens no port. UCI, XBoard and CNP over stdio behave as they do.
- **Loopback by default.** Any other address needs explicit configuration and a security model.
- **A thin adapter.** Networking lives outside `crates/chess` and `crates/engine`
  (CLAUDE.md, rule 1).
- **Search is untouched.** The bench stays 767812 and the change says "No functional change".

There is **no SSH** in this design. A customer product can't ask people to set up SSH keys and a
shell account, so cb2026 provides the encryption and the authentication itself.

## What exists, and what is missing

- `crates/cnp` serves CNP over stdin and stdout: initialisation and capabilities, search with
  progress, stop and cancel, and structured errors. A node runs **one search at a time**
  (`maxConcurrentSearches` 1) and answers a second with `Busy`.
- `cb2026 --cnp` took **no thread or hash settings**, so a remote engine would have used one
  thread and 16 MB, and a fast remote machine would have done the work of a slow one. **Step 1
  fixed that** (below): `cb2026 --cnp --threads N --hash MB`.
- Options that name a file (`EvalFile`, `SyzygyPath`, `BookFile` in UCI) must never be settable
  by a remote client. **Checked in step 1: no CNP request can name a file.** A search takes a
  position, its history, limits, the strength level, the techniques' switches by name, MultiPV,
  and a few flags (`deterministic`, `ponder`, `chess960`); the network, the book and the
  tablebases are fixed by how the node was started. So the allowlist the listener needs is the
  set of requests CNP already has, and a new CNP option that takes a path must not be added
  without a decision.

## The design

### Shape: a `crates/net` adapter

A new crate, `crates/net`, beside `crates/cnp`: no chess logic, `#![forbid(unsafe_code)]`, and it
only moves CNP lines between a connection and the existing CNP node. It has three parts:

1. **A connection** (an encrypted, authenticated stream of lines) that doesn't care which side
   opened it. The first release only **accepts**; a later release can **dial out** with the same
   code (see stage 2 below).
2. **A trust layer**: certificates, pairing and the list of approved clients.
3. **A limiter**: connections, message sizes, deadlines and resource caps.

The CLI gets a mode, provisionally `cb2026 --serve [ADDRESS:PORT]`. The final spelling is part
of Decision 4 below.

### Security model (Decision 1)

The listener serves CPU to whoever may connect, so it has to know both who it is and who is
asking, with no certificate authority and nothing to install:

- **Encryption:** TLS 1.3 (rustls). The engine makes a **self-signed certificate on first run**
  and keeps the key in its own data directory with owner-only permissions.
- **Pairing:** to approve a client, the person starts `cb2026 --pair` (or the same action from a
  future UI) on the engine's machine. It shows a **fingerprint and a one-time code** that expires
  in a few minutes. The client connects, checks the fingerprint (the person confirms it matches),
  sends the code, and presents its own certificate. The engine stores that client's fingerprint
  in an **approved-clients file**. From then on both sides check each other's fingerprint, with
  no code and no prompt.
- **Revoking** a client is deleting its line from the file.
- Anyone else is refused before CNP starts, and a refusal says nothing about why.

**As built (step 3):**

- TLS 1.3 only, through rustls with the `ring` backend. Each side's certificate is self-signed
  and made on first use (ECDSA P-256). The handshake's signature is checked in both directions,
  so presenting someone's public certificate without its key gets nowhere (tested with a
  certificate and a key that don't belong together). What a certificate *means* is only its
  SHA-256 fingerprint, checked after the handshake.
- **Files**, in one directory (`~/Library/Application Support/cb2026` on macOS,
  `$XDG_DATA_HOME/cb2026` or `~/.local/share/cb2026` on Linux, `%APPDATA%\cb2026` on Windows;
  `--data-dir` chooses another), readable by the owner alone on Unix: `identity.pem`,
  `approved-clients.txt` (`fingerprint date name`, one client a line, hand-editable, read at
  each connection so a revoked client is refused at its next one), and `pairing.txt` (present
  only while a window is open). On Windows the application-data folder is already the user's
  own, and no extra mode is set.
- **`cb2026 --pair`** is a separate process from `cb2026 --serve`; they meet only in those
  files. It prints the fingerprint and an **eight-digit code**, valid for **5 minutes**, for
  **one** client, closing after **3 wrong tries**; then it waits until a client has paired or the
  window has closed (exit code 0 or 1).
- **What a client must do** is written in `crates/net/src/client.rs`, which is also the client the
  tests use: connect without knowing the node, **show the person the node's fingerprint and wait
  for them to say it matches the one on the engine's screen**, then send the code. That
  comparison is the defence against someone in the middle; the code alone doesn't give it.
- A client that isn't approved can send **one** message of at most 1 KiB, the pairing request,
  and only that. Anything else, a wrong code, no open window: the connection is closed with no
  reply, so a guesser learns nothing.
- Handshakes happen on their own threads, at most 16 at once (more are dropped), and **only an
  approved client reaches the one place**: a stranger can't make an approved client wait or be
  told `Busy`.
- `--serve` on a loopback address stays plain (for a program on the same computer); on any other
  address, or with `--tls`, it is TLS with approved clients only. There is no plaintext mode off
  loopback.

This is the recommendation. The options, with their costs:

| Option | Pro | Con |
| --- | --- | --- |
| **Pairing, then both sides pin each other** (recommended) | Nothing to type after the first time; no shared secret on the wire after pairing; a stolen token isn't a skeleton key | A pairing step; the engine keeps a small file of approved clients |
| A shared token over TLS | The least to build | One secret opens everything; people paste tokens into the wrong place; the server certificate still has to be trusted somehow |
| Mutual TLS from a certificate authority | The usual enterprise answer | A CA is a lot to ask of a home user, and not needed for a LAN product |

### Clients (Decision 2)

**One client at a time** in the first release, which matches CNP's one search at a time. A
second connection from an approved client gets `Busy` and a close. This keeps memory predictable
(one hash table) and the isolation rules simple. Isolated per-client engines could follow, with
their own memory cap; nothing here prevents them.

Connection rules, in every release:

- Request IDs belong to a connection, so reusing one on another connection means nothing.
- A client can't read another's responses, cancel its work, change its options or stop the
  service. `shutdown` is a local command, not a CNP one.
- **A disconnect cancels the search**, and nothing resumes. No response ever goes to a different
  connection or a different request.

### Finding the engine (Decision 3)

**Both ways to find an engine are supported** (Carlos, 2026-11-05): **entering an IP address
and port**, and **automatic discovery by UDP broadcast**. The typed address is a full feature,
for people who know their network and for networks where a broadcast can't reach; discovery is
the easy way for people who are less technical, who should never have to find an address. A
client offers both, with discovery as the first thing a new user sees. Discovery only says
*where an engine is*; it is never trusted. Pairing and the pinned fingerprints (above) decide who
may connect, the same way however the engine was found.

- **Opt-in, in two layers.** Discovery runs only when the engine is serving on a non-loopback
  address **and** is started with `--discover`. A listener with no flag answers no probes. For
  the less technical user the flag isn't the interface: the setup screen of a client or
  installer turns it on with the same choice, and the documentation shows both.
- **The engine answers, it doesn't announce.** A client sends a small *probe* to the broadcast
  address on a fixed UDP port; an engine that has discovery on answers the sender directly with
  one small JSON line: the node's name, the protocol version, its TCP port, its certificate
  fingerprint and whether it is accepting pairing. No timers, no periodic chatter, nothing on
  the wire until someone asks.
- **No amplification.** A reply is never larger than the probe that asked for it (probes are
  padded to a fixed size), and an engine answers at most a few probes a second per sender.
- **Nothing private in it.** No hostname, user name, path, CPU model or memory size: the name
  is one the owner set, and the fingerprint is the engine's certificate's, which a TLS
  handshake reveals anyway. A client that isn't paired learns only that an engine is there.
- **A hint, not a credential.** A client connects to the address a reply came from, then checks
  the fingerprint against its own approved list, or asks the person to confirm it (pairing). A
  forged reply can only send a client to a machine that then fails the TLS check.
- **Where it works, and where it doesn't.** A broadcast reaches the local network segment only.
  It won't cross a router or VLAN, many Wi-Fi access points isolate clients from each other, and
  a firewall may block the UDP port, so a person can always enter the address instead. IPv6 has no
  broadcast, so the first release is IPv4; an IPv6 multicast group can be added with the same
  messages.
- **Every interface.** Sending to the all-ones address (`255.255.255.255`) reaches only one
  interface on some systems, so a client probes each interface's own broadcast address. Listing
  the interfaces needs either a small dependency (checked by `cargo deny`, like the TLS stack)
  or the operating system's own calls; that is settled in the step's PR.
- **Operating-system prompts.** macOS and Windows ask the person once to allow incoming
  connections for a program that listens; the documentation says so, with what to click.

The probe and the reply, with the port number, are written into the CNP specification's
transport section (step 6), so another client can use them. A standard like mDNS would find
engines across more networks and is the alternative; it adds a service to the machine, and it can
replace this without changing the protocol above it.

### Limits and the server's caps (Decision 4)

Set by the owner of the engine's machine, as flags or a small config file, and never raised by a
client:

| Limit | Provisional default | Why |
| --- | --- | --- |
| Threads a search may use | `--threads N` (step 1); 1 if not given | A request carries no thread count, so a client can neither raise nor lower it |
| Hash memory | `--hash MB` (step 1); 16 MB if not given | The same |
| MultiPV | the engine's existing cap | |
| One message | 1 MiB, `cb2026_net::MESSAGE_BYTES` (step 2); a stdio node reads 16 MiB | Bounded framing; a position with its whole history is a few kilobytes. A longer message is refused with an error and the node says the limit in `initialize` |
| First message of a connection | 10 seconds, for the whole message (step 2) | A client that connects and trickles bytes is dropped |
| One write to a client | 30 seconds (step 2) | A slow reader can't stall a search |
| A client that vanished | TCP keepalive: 60 s of silence, then probes every 10 s (step 2) | See below: there is **no idle timeout** |
| Search time | none beyond what the client sends | The client owns its clock |
| Connections waiting | 0 (a second one is told `Busy` and closed) | |

**Why no idle timeout.** The first draft had one (5 minutes). Building it showed it can't be
done from outside the node: a client waiting for a long search sends nothing for as long as the
search runs, and the listener can't tell that silence from idleness, so a timeout would kill
searches. A client that has *gone* (its network dropped, no goodbye) is the real danger to the one
place, and keepalive finds it. A client that is connected, alive and idle may keep its place.

The CNP node reports the effective message limit in `initialize` (`maxMessageBytes`), and, since
step 6, the threads and hash it runs with, in a new optional `resources` object
(`{"threads": 8, "hashMb": 1024}`), added to the CNP specification for this (section 5.1).

### Where it lives (Decision 5)

A separate crate, `crates/net`, so the TLS stack (and its licences) stays out of the crates that
hold chess logic, and out of builds that don't serve. It is a default-off cargo feature of the
CLI, so the usual release binary keeps its dependencies unless we decide a release carries it.
That is a separate choice, made when the feature is ready.

## What can go wrong, and what handles it

- **A port left open to the world.** Loopback by default. A non-loopback address needs the
  explicit address **and** gets TLS with pairing automatically; there is no plaintext mode off
  loopback, and the plaintext mode refuses any other address.
- **A flood of strangers.** At most 16 connections are greeted at once overall and **4 from one
  address**, and each has 10 seconds to finish; more are closed unanswered. An address with
  **8 failed greetings in 10 seconds is ignored for 30**. A stranger never reaches the one
  place, so a flood can't make an approved client wait or be told `Busy`. It can still delay
  its connection if the flood comes from the same address (a person and an attacker sharing one
  address, or one household's router seen from outside: one allowance for all of them), which is
  the price of asking nothing of the person; the tables forget the oldest address past 1,024.
- **Slow or hostile input:** bounded line length, read and write deadlines, backpressure when a
  reader is slow, and positions and options validated before any search starts.
- **A client picks a file:** remote options are an allowlist. Paths (`EvalFile`, `SyzygyPath`,
  `BookFile` and the like) can only be set by the person at the engine's own command line.
- **Cancellation races:** stop followed at once by a new search, and a disconnect during a
  search, are tested, and no stale move reaches a later request.
- **Credentials in logs:** fingerprints may be logged; pairing codes and keys never are.
- **Shutdown:** stop accepting, cancel or finish active searches within a bound, close
  connections, join threads.

## Compatibility with stage 2

Solrac's distributed-search plan has each machine connect **out** to the program that schedules
the work, with a supervisor on each machine running a pool of engines. This design keeps that
easy:

- **The connection doesn't care who opened it**, so dialling out is a small addition.
- **The trust layer is general**: pairing and pinned fingerprints serve both a person approving
  a GUI and a machine joining a cluster.
- **The pool isn't in the engine.** A supervisor can start single-session engines over stdio
  and carry them over this transport; "one client at a time" never limits a pool.
- **`initialize` already says what a machine has** (build, network, threads, hash), the facts a
  scheduler wants.
- **Nothing resumes**, but nothing forbids a later lease or attempt ID, which is additive.

## Dependencies and licences

cb2026 is `MIT OR Apache-2.0`, so every dependency must pass `cargo deny check licenses`
(CLAUDE.md, rule 2). A TLS stack is the first dependency with a notable licence story, so it
is checked before it is chosen:

| Crate | Declared licence |
| --- | --- |
| `rustls` | Apache-2.0 OR ISC OR MIT |
| `rustls-webpki` | ISC |
| `ring` | Apache-2.0 AND ISC |
| `aws-lc-rs` | ISC AND (Apache-2.0 OR ISC) |
| `aws-lc-sys` | ISC AND (Apache-2.0 OR ISC) AND Apache-2.0 AND MIT AND BSD-3-Clause AND … |

(Read from each crate's `Cargo.toml` on 2026-11-05.) Every licence on this list is already in
`deny.toml`'s allow list. Two things to do before the dependency lands:

- **Run `cargo deny check licenses` with the choice added.** Done in step 3 with `rustls`
  (`ring` backend), `rcgen` (`ring`, `pem`), `sha2` and `serde_json`: `cargo deny check` passes
  (advisories, bans, licences, sources). No `rustls-pemfile` (unmaintained): rustls's own
  `pki-types` reads the PEM.
- **Settle the OpenSSL attribution.** **Chosen: `ring`**, not AWS-LC. `ring`'s licence files
  (`LICENSE`, `LICENSE-BoringSSL`, `LICENSE-other-bits`) don't name OpenSSL (it appears in
  comments in a few source files only), and `scripts/notices.sh` runs with it and includes
  `ring`'s and BoringSSL's licence texts in `THIRD-PARTY-NOTICES.txt`. That is my reading of the
  files, not legal advice; the release that carries the feature should have the notices file
  looked at once.
- **Still to check:** that `ring` builds on all four release targets (it compiles C and
  assembly). Linux CI and the M1 Max build today; Windows and Intel macOS are checked by the
  Cross-platform workflow, which hasn't run with the `net` feature.

The licence of a client that links none of this stays as it is: a program that only talks to
the listener over a socket takes no licence from it.

## How it would be built

Each step is its own PR with a green CI, and none changes the search:

1. **Threads and hash in CNP mode** (done 2026-11-05): `cb2026 --cnp --threads N --hash
   MB`, N from 1 to 256 and MB from 1 to 65,536 as UCI's options are, refused with exit code 2
   with any other mode. Without them a node keeps one thread and 16 MB. **A search request
   can't change them**: the node's owner chooses them, which is the cap the design wants.
   `initialize` reports them (step 6: `resources` in the specification).
2. **`crates/net`: framing, limits and a loopback listener** (done 2026-11-05): plaintext on
   loopback only (any other address is refused), the one-client rule with `Busy` for the rest,
   a fresh engine for each client, a disconnect cancelling the search, a client's `shutdown`
   and `exit` ending only its own session, the deadlines above, and end-to-end tests over real
   loopback connections. `cb2026 --serve [ADDR]` runs it, in a build with `--features net`
   (off by default); stop it with Ctrl-C. It is the first thing that opens a port, and only
   when asked, and only on this computer.
3. **TLS, certificates and pairing** (done 2026-11-05): rustls (`ring`), self-signed
   certificates made on first use, `cb2026 --pair`, the approved-clients file, and
   `cb2026 --serve ADDR` on any address (encrypted, approved clients only), as in
   [Security model](#security-model-decision-1) above. A reference client
   (`cb2026_net::client`) and 17 end-to-end tests over real TLS connections, which include the
   attacks listed there; breaking the signature check, the approval check or the deadline each
   makes tests fail. **Not yet run:** the `net` feature on Windows and on an Intel Mac (CI
   builds Linux only), and a connection between two computers.
4. **Discovery** (done 2026-11-05): `cb2026 --serve ADDR --discover [--name NAME]`, only with
   the encrypted listener. **UDP port 4026** (the TCP default's number; a different protocol),
   a **probe of exactly 320 bytes** (JSON padded with spaces) and a **reply** of one JSON line
   with the name (at most 32 bytes), the CNP version, the TCP port, the certificate fingerprint
   and whether a pairing window is open: never larger than the probe (tested with the longest
   name, in the widest characters), at most **4 replies a second to one sender and 40 in all**,
   a sender table bounded at 1,024, and no answer to a datagram of any other size or content. The
   socket is opened before serving starts, so a port in use is reported at once. The client side,
   `cb2026_net::client::discover`, probes each interface's broadcast address (found with the
   `if-addrs` crate, MIT/Apache-2.0-class licence, passes `cargo deny`) and `255.255.255.255`,
   sends the probe twice, ignores malformed replies, and returns what answered: a hint to check
   with the fingerprint, never trust. The default name is `cb2026`, not the computer's name.
   Tests: 10 end-to-end over real UDP on loopback, and unit tests for the messages and the
   limiter; breaking the size rule or the rate limit each fails a test. **Not tested:** a real
   broadcast across a network (it can't be tested portably; the messages are the same), Windows
   and Intel macOS, and the operating systems' firewall prompts.
5. **Hardening** (done 2026-11-05): the per-address limits and ban above (`crates/net/src/abuse.rs`),
   and 14 end-to-end negative tests (`crates/net/tests/hardening.rs`), most run on both the
   plaintext and the TLS listener: a message sent two bytes at a time, malformed JSON of many
   kinds (including 100,000 nested brackets and a 10,000-digit id), bytes that aren't text,
   oversized messages, 14 invalid positions, moves and limits and an absurd `multipv`, a cancel
   followed at once by a new search (10 rounds: the cancelled search never answers with a move),
   request ids reused by the next connection (nothing stale reaches it), half a message then a
   disconnect, a client that never reads (the listener still stops), 150 silent connections from one
   address, an address that keeps failing, a client with no certificate, and a plain listener
   refusing `0.0.0.0` and `[::]`. Breaking the per-address cap or the ban each fails a test.
   **Findings:** none that needed a change to the CNP node: it answered every malformed message
   with an error and went on. **By design, not fixed:** an approved client that stalls half-way
   through a message keeps its place until it disconnects or keepalive finds it dead (there is no
   idle timeout, because a long search is silent too); another approved client is told `Busy`.
   **Not done:** fuzzing the listener's input with `cargo fuzz`, and a test across a real network.
6. **Documentation and the specification** (done 2026-11-05): book chapter 22 ("Network play"), a
   README section, and, in the Chess Node Protocol repository (cjus/chess-node-protocol#24),
   section 3.2 (TLS over TCP, pairing, one client at a time, deadlines), section 3.3 (the
   discovery probe and reply), section 10, and `resources` in `initialize`'s result, with a test
   that checks the spec's transport examples. The wire messages use the key `"cnp"`, not
   `"cb2026"`, so that another node or client can use them. The licence notices need no change
   (`scripts/notices.sh` includes `ring`, rustls and the rest). The spec's additions are optional
   and went into the 1.0 draft as pondering did; Carlos can ask for 1.1 instead.
   **Waiting on Carlos:** a try between two real computers and the decision whether releases carry
   the feature (#263).
   **`cb2026 --connect [ADDRESS]`** (added 2026-11-05, after the Solrac side asked for a way to
   test before it has a window): a command-line client in `crates/cli/src/connect.rs` over
   `cb2026_net::client`. It finds a node (discovery, or an address; port 4026 if none), shows the
   fingerprint and sends nothing until the person types yes, asks for the code, keeps the
   node's fingerprint in `engines.txt` and its own identity in `client-identity.pem`, then
   runs `initialize` and a one-second search. A node it has paired with needs no typing. 4
   end-to-end tests (`crates/cli/tests/serve.rs`): pair then reconnect, declining sends no
   code, a wrong code remembers nothing, nothing listening; making "no" send the code fails
   the declining test.
7. **Later, if wanted:** dial-out mode, isolated per-client engines, IPv6 multicast and mDNS.

The Solrac side (a client connection, the pairing screen, the engine list, the discovery probe)
is separate work and starts once step 3 has a protocol to talk to.

## Decisions

Answered by Carlos on 2026-11-05 (cjus/chessengine#255):

1. **Security model:** pairing, with both sides pinning each other.
2. **Clients:** one at a time.
3. **Discovery:** **both**: entering an IP address and port, and automatic discovery by UDP
   broadcast for less technical users (a change from the typed address only that was
   recommended), described above.
4. **Limits:** set by flags or a config file on the engine's machine, with the provisional
   defaults above.
5. **Where it lives:** a separate `crates/net`, off by default in the CLI.
