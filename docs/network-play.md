# Network play: a design for the opt-in listener (#209)

**Status: a proposal, not built.** Nothing here opens a port today. It waits on Carlos's answers
to the five decisions below (cjus/chessengine#255), and no code
starts before then. Written 2026-11-05.

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
- `cb2026 --cnp` takes **no thread or hash settings**, so a remote engine would use one thread and
  16 MB. That has to change first, or a fast remote machine does the work of a slow one.
- Options that name a file (`EvalFile`, `SyzygyPath`, `BookFile` in UCI) must never be settable
  by a remote client. Which of CNP's options can name a path is checked in step 1.

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

**A typed address and port** in the first release. Automatic discovery (mDNS) is useful, but it
adds a service to the machine, a platform-specific prompt and a way to see engines that haven't
paired. It can come later without changing the protocol.

### Limits and the server's caps (Decision 4)

Set by the owner of the engine's machine, as flags or a small config file, and never raised by a
client:

| Limit | Provisional default | Why |
| --- | --- | --- |
| Threads a search may use | all but one of the machine's performance cores | A client may ask for fewer, never more |
| Hash memory | 1/4 of RAM, at most | A client may ask for less |
| MultiPV | the engine's existing cap | |
| One message | 1 MB | Bounded framing; a position and its history fit easily |
| Idle time with no message | 5 minutes | Read and write deadlines are separate and shorter |
| Search time | none beyond what the client sends | The client owns its clock |
| Connections waiting | 0 (a second one is refused) | |

A client's thread and hash requests are clamped to these. The CNP node reports the effective
values in `initialize` (build, network hash, threads, hash caps), so the client sees what it
got, and Solrac's stage 2 can read the same fields.

### Where it lives (Decision 5)

A separate crate, `crates/net`, so the TLS stack (and its licences) stays out of the crates that
hold chess logic, and out of builds that don't serve. It is a default-off cargo feature of the
CLI, so the usual release binary keeps its dependencies unless we decide a release carries it.
That is a separate choice, made when the feature is ready.

## What can go wrong, and what handles it

- **A port left open to the world.** Loopback by default. A non-loopback address needs the
  explicit flag **and** TLS with pairing; there is no plaintext mode off loopback. The tests'
  loopback plaintext mode refuses any other address.
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

- **Run `cargo deny check licenses` with the choice added.** Nothing is added yet, so this has
  not been run.
- **Settle the OpenSSL attribution.** AWS-LC descends from BoringSSL and OpenSSL, and its
  LICENSE file names OpenSSL, though the declared expression doesn't list that licence. Whether
  `THIRD-PARTY-NOTICES.txt` must also carry OpenSSL's notice is a question for the notices tool or
  for counsel. `ring` is the alternative backend. Either choice also needs a check that it
  builds on all four release targets, since both compile C.

The licence of a client that links none of this stays as it is: a program that only talks to
the listener over a socket takes no licence from it.

## How it would be built

Each step is its own PR with a green CI, and none changes the search:

1. **Threads and hash in CNP mode** (`--threads`, `--hash`, and what `initialize` reports),
   and which CNP options can name a path. Useful on its own, and it is the first thing a
   remote engine needs.
2. **`crates/net`: framing, limits and a loopback listener** with plaintext allowed only on
   loopback, the one-client rule, cancellation and shutdown, and the loopback end-to-end tests.
3. **TLS, certificates and pairing**, with the approved-clients file; the non-loopback address
   turns on here and not before.
4. **Hardening tests:** the negative tests in #209's verification list (unauthenticated access,
   oversized, fragmented and malformed messages, slow input and output, invalid positions and
   options, connection exhaustion), each run with and without TLS where it applies.
5. **Documentation:** a chapter in the book, a README section, a transport section for the CNP
   specification, and the licence notices.
6. **Later, if wanted:** dial-out mode, isolated per-client engines, mDNS.

The Solrac side (a client connection, the pairing screen, the engine list) is separate work and
starts once step 3 has a protocol to talk to.

## Decisions for Carlos

1. **Security model:** pairing with both sides pinning each other (recommended), a shared
   token over TLS, or certificates from a certificate authority?
2. **Clients:** one at a time (recommended), or an isolated engine for each?
3. **Discovery:** a typed address only (recommended), or mDNS as well?
4. **Limits:** set by flags or a config file on the engine's machine (recommended), and the
   provisional defaults above?
5. **Where it lives:** a separate `crates/net`, off by default in the CLI (recommended), or inside
   `crates/cnp`?
