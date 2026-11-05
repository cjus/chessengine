# Network play

*Opt-in, off by default · code in `crates/net`, `crates/cnp` and `crates/cli/src/main.rs` ·
the protocol's rules are section 3 of the Chess Node Protocol specification*

Everything so far ran on one computer. This chapter lets one computer use another's processors:
you run cb2026 on a fast machine, and a chess GUI on your laptop plays with it. It is also the
chapter where cb2026 first **opens a port**, so most of it is about who gets to use that port,
and what happens when someone who shouldn't tries.

Nothing here changes how the engine searches. The listener is a thin adapter: it moves lines of
the Chess Node Protocol (CNP, from the previous chapters' JSON mode) between a network
connection and the same `cb2026-cnp` node that serves a program started on standard input. A
search means exactly what it meant before; this crate only decides who may talk to the node,
when, and for how long.

## The problem with an open port

A listening program is a door. On your own network the people who might knock are your
family's phones, a guest's laptop and anything that got onto the Wi-Fi by accident. And the
door is expensive: whoever gets in can ask your processors to search for as long as they like.

The usual answers don't fit a program people download. SSH needs accounts and keys. A password
typed into the GUI and sent over the network has to be sent *somewhere safe*, and the program
has nothing to make a connection safe with. A certificate authority is what browsers use, and
asking a home user to run one is asking too much.

So cb2026 does the whole job itself, with nothing to install and nothing to type twice.

## Encryption without an authority: pin the fingerprint

The connection is **TLS 1.3**, the same encryption as `https`, with two unusual choices.

**Both sides have a certificate, and both are self-signed.** The first time cb2026 serves, it
makes a certificate and keeps it, with its key, in its own folder. A client makes one too. A
certificate that anyone can make proves nothing about *who* made it, so cb2026 doesn't ask what
is in it. It asks for its **fingerprint**: the SHA-256 of the certificate's bytes, written as 32
pairs of hex digits:

```text
3F:A9:0C:... (32 pairs)
```

A fingerprint can't be faked without the private key, because the TLS handshake includes a
signature that only the key's holder can make. cb2026 checks that signature in both
directions. This matters more than it sounds: certificates cross the wire in every handshake,
so anyone watching the network has your approved client's certificate. Without the signature
check, they could present it as their own. One of the tests builds exactly that attack, a
copied certificate with a different key, and requires the handshake to fail; breaking the
check on purpose makes that test fail, which is how we know the test looks at the right thing.

What the fingerprint *means* is up to each side's memory. The client remembers the engine's,
and refuses any other. The engine keeps a list of the clients' fingerprints it has approved,
and closes the connection of any other. Nobody trusts a name; everybody trusts a thing they
have seen before. Browsers call this "trust on first use", and SSH's `known_hosts` is the same
idea. The only hard moment is the *first* time, which is pairing.

## Pairing: one code, and one comparison

The first time a client connects, the engine doesn't know it. A person who is standing at the
engine's computer lets it in:

```text
$ cb2026 --pair
Pairing: approve one new client.

1. On the client, choose this computer. It shows a fingerprint; check that
   it is the same as this one:

     3F:A9:0C:5E:...
     ...

2. Only if they match, type this code into the client:

     4821 0937

The code works once and for 5 minutes, and closes after 3 wrong tries.
```

There are two secrets here and they defend against different people.

- **The code** proves that someone at the engine's keyboard let *this* client in. A stranger on
  the network can try to pair, but has to guess eight digits, and the window closes after three
  wrong tries, so a guesser has roughly three chances in a hundred million.
- **The fingerprint comparison** proves the client is talking to *this* engine and not to a
  program in the middle that is relaying everything. Such a program can't produce the engine's
  certificate, so its fingerprint differs; if the person compares the two screens, the attack
  is visible. A client that sent the code first and compared later would hand the code to the
  attacker. The specification therefore says the client must show the fingerprint and wait for
  the person before it sends the code.

After pairing the client's fingerprint is a line in a text file, `approved-clients.txt`.
Revoking a client means deleting its line, and it is refused at its next connection, with no
restart, because the listener reads the file for every connection. `cb2026 --pair` and
`cb2026 --serve` are separate programs that only share those files.

A client that isn't on the list may send exactly one thing, a pairing request of at most 1 KiB.
Anything else, a wrong code, no open window, gets the connection closed with **no reply**. A
helpful error message ("wrong code", "not approved") would tell a guesser which part to
work on.

## One client at a time

CNP runs one search at a time, so the listener serves one client at a time. A second approved
client is told `Busy` and disconnected. Each client gets a **new engine**: nothing of the last
client's hash table, options or game survives. When a client's connection ends for any reason
the search is cancelled, and nothing resumes on the next connection, so an answer can never
reach the wrong person.

That rule has a consequence that is easy to miss. Strangers shouldn't be able to occupy the one
place, and a handshake takes time, so connections are *greeted* on threads of their own, and
only a client that is approved reaches the place at all. A stranger can waste a few threads for
a few seconds, and the next section is about stopping even that.

## The timeout we couldn't have

The first design had an idle timeout: close a connection that says nothing for five minutes.
Building it showed it could not exist. A client waiting for a long search sends nothing for as
long as the search runs, and from the listener's side that silence is identical to a client
that walked away. A timeout would kill real searches.

What the listener can do is attack the two cases that really hurt:

- A connection that **never gets going** has ten seconds to finish its handshake and send its
  first message. The ten seconds cover the whole thing, not each read, because a client that
  sends one byte every nine seconds would otherwise stay forever. The limit is applied where the
  bytes come in (a small wrapper around the socket's read), so it holds however many reads TLS
  needs to assemble one record.
- A client that **vanished** (its Wi-Fi dropped, nothing said goodbye) is found by TCP
  keepalive, after about a minute of silence and a few probes.

A client that is connected, alive and silent may keep its place. That is by design, and a test
says so: another approved client is told `Busy`.

## Limits that don't ask permission

Whoever owns the engine's computer sets the limits (`--threads`, `--hash`, and the numbers
below); a client can't raise them, and CNP has no request that can change them. The engine now
*says* them in `initialize`, in an optional `resources` object, so a program choosing among
several machines can tell one with 64 threads from one with 1.

| What | Limit |
| --- | --- |
| One message | 1 MiB (a stdio node reads 16) |
| First message | 10 s, the whole message |
| One write to a client | 30 s |
| Connections greeted at once | 16 in all, 4 from one address |
| An address that fails 8 times in 10 s | ignored for 30 s |

The tests for these send what an attacker would: a message two bytes at a time, 100,000 nested
brackets, a ten-thousand-digit id, bytes that aren't text, a message past the limit, a flood of
150 silent connections, a client that never reads the answers (the listener must still stop),
a cancel followed at once by a new search (the cancelled search must never answer with a
move). None of them needed a change to the CNP node, which answered every bad message with an
error and carried on. The one thing they found is the stalled client above, which holds the
place by design.

## Finding the engine: a question shouted at the network

Typing `192.168.1.23:4026` is fine for people who know their network. For everyone else, a
client can **ask the network** whether a cb2026 is there. It sends a small UDP datagram to the
broadcast address, which every machine on the local network receives, and any engine that has
been told to (`--discover`) answers.

```text
→ {"cnp": "probe", "v": 1}                         (padded to 320 bytes)
← {"cnp": "here", "v": 1, "name": "the study", "protocol": "1.0", "port": 4026,
   "fingerprint": "3F:A9:...", "pairing": false}
```

Shouting at a network invites a particular abuse. UDP has no handshake, so the sender's address
in a datagram can be forged. Someone could send probes that claim to come from a victim, and
every engine would send its answer to the victim. If the answer were bigger than the probe,
that would multiply the attacker's traffic. So the probe is padded to a fixed 320 bytes and a
reply is never longer, and an engine answers at most four probes a second to one address.
Datagrams of any other size get nothing at all.

Nothing private is in the answer: no computer name, no user name, no hardware. The name is one
the owner chose. And the answer is **a hint, not a credential**: it says where an engine
*claims* to be. A client connects there and checks the fingerprint the way it always does, so a
forged answer can only send a client to a machine that fails the check.

Broadcast has limits you should know. It reaches only the local network segment; many Wi-Fi
routers stop devices seeing each other; a firewall can block the port. That is why typing an
address stays a full feature, and why a client must always offer it.

## Try it

You need a cb2026 built with `--features net` (the release downloads don't have it yet; the
chapter's code is off by default):

```text
cargo build --release -p cb2026 --features net
```

On the fast computer:

```text
cb2026 --serve 0.0.0.0:4026 --discover --name "the study" --threads 8 --hash 2048
```

It prints its fingerprint. In another terminal on the same computer:

```text
cb2026 --pair
```

Then, from the other computer, a client finds the engine (or you type its address), shows you the
fingerprint, and asks for the code. The reference client in `crates/net/src/client.rs` is a short
file and is what the tests use: read it to see exactly what a client does.

Your operating system may ask once whether to allow incoming connections for the program; say
yes for private networks.

## Exercises

1. Run `cb2026 --pair`, then pair a client with a wrong code three times and watch the window
   close. What does the client see each time? What would a client learn if the engine said
   "wrong code" instead?
2. Open `approved-clients.txt` after a successful pairing and delete the line. Reconnect with
   the same client. Why doesn't the engine need restarting?
3. The probe is 320 bytes and the reply must never be longer. Change the reply's longest field
   and find the test that notices. What is the longest name that fits, in bytes?
4. Find the test that connects 150 times from one address. Why does it count the connections
   that are *still open* rather than the ones that were refused?
5. There is no idle timeout. Write down a way to add one that doesn't kill a long search.
   (Hint: the client can say that it is waiting.) What would the protocol need?
