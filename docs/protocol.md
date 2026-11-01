# The SweaxDrop protocol (`swx/1`)

This is the wire protocol every SweaxDrop app speaks: the Windows app, the
Android app, the iOS app and the headless benchmark tool. It is implemented
once, in Rust (`crates/core`), and the apps embed that code. Numbers and
names below come from the source; file and line references are given where
it helps.

## 1. Identity

Every installation creates one Ed25519 key pair on first start and wraps it
in a self-signed X.509 certificate (`identity.rs`). Nothing else identifies a
device:

* **fingerprint** = SHA-256 of the raw 32-byte Ed25519 public key
* **device id** = the first 80 bits of the fingerprint in base32, 16
  characters, shown as `K7Q2-9MXA-L4TB-N8ZE`

Renaming the device does not change its id. A peer can only present an id if
it holds the matching private key, because the id is checked against the key
proven in the TLS handshake (section 3).

## 2. Discovery

Three mechanisms run at the same time (`discovery.rs`). None of them is
trusted; they only tell a device where to connect.

| | Address | Payload | Notes |
|---|---|---|---|
| Multicast | `239.255.71.71:47471` (UDP) | JSON announce | queries are answered at once, so a new device sees the others in one round trip |
| Broadcast | each interface's directed broadcast and `255.255.255.255`, port 47471 | same | for routers and phones that drop multicast |
| mDNS / DNS-SD | `_sweaxdrop._tcp.local.` | TXT: `id`, `name`, `plat`, `v` | what iOS may browse without Apple's multicast entitlement |

Announce packet:

```json
{"swx":1,"id":"K7Q29MXAL4TBN8ZE","name":"Desk PC","plat":"windows","model":"","port":47470,"q":false,"v":"1.0.0"}
```

`q: true` asks everyone to answer. Devices are re-announced after 1, 2 and 3
seconds, then every 4 seconds for a minute, then every 12 seconds. A device
not heard from for 40 seconds shows as offline.

Addresses are ranked: USB tethering subnets first (`192.168.42.x`,
`192.168.44.x`, Windows ICS `192.168.137.x`), then direct links (Android
hotspot `192.168.43.x`, Wi-Fi Direct `192.168.49.x`), then the LAN. When a
device has several addresses, connection attempts start 200 ms apart and the
first to finish the TLS handshake wins (`send.rs`, `connect_any`).

## 3. Transport and encryption

One TCP port (default 47470, the next free one otherwise) carries two
protocols, told apart by the first byte (`node.rs`, `handle_tcp`):

* `0x16` (a TLS ClientHello) → the encrypted device-to-device protocol
* anything else → plain HTTP for the browser client (section 8)

The same port number on UDP is a QUIC endpoint (quinn), which can carry the
same protocol. TCP is the default because it measured faster on every case
we tried (docs/benchmarks.md).

**TLS 1.3 only**, mutual authentication, ALPN `swx/1` (`tls.rs`):

* both sides present their Ed25519 certificate
* the certificate verifiers accept any well-formed Ed25519 certificate and
  check the handshake signature; which key is *acceptable* is decided above
  TLS: the id announced in `Hello` must equal the id derived from the key in
  the handshake, and a trusted device's stored fingerprint must match exactly
  (`send.rs`: "this device presented a different key than expected")
* cipher order: AES-128-GCM, ChaCha20-Poly1305, AES-256-GCM (ChaCha20 can be
  preferred in settings for phones without AES instructions)
* no session tickets are sent: data streams never read, and on Windows a
  socket closed with unread bytes is reset (RST), which can cut off data in
  flight

## 4. Framing

**Control frames**: `u32` little-endian length, then a JSON object tagged by
`"m"`. Maximum 64 MiB (an offer with a million files fits).

**Chunk frames** on data streams (`wire.rs`):

```
0xC1 | item u32 | index u32 | raw_len u32 | flags u8 | wire_len u32 | bytes[wire_len] | blake3[32]
0xE0                                                    end of stream
```

All integers little-endian. The hash is BLAKE3 of the raw bytes. A chunk's
offset in its file is `index × chunk_size`; the receiver checks that
`raw_len` is exactly what the offer implies for that index.

## 5. A transfer

```
sender                                     receiver
  ── TLS handshake (both keys) ───────────────▶
  Hello {proto:1, device}           ─────────▶
                                   ◀───────── Hello {proto:1, device}
  Offer {transfer, items[], total, chunk, text?, resume}
                                   ─────────▶   ask the user, or accept
                                   ◀───────── Accept {token, streams, have[], skip[]}
  open N data streams, each:  Attach {token}, then chunk frames … 0xE0
  AllSent {streams: N}             ─────────▶
                                   ◀───────── Receipt {ok, failed[]}
```

* **Items**: `{id, path, size, mtime?, mime?, dir?}`. Paths are relative,
  `/`-separated; folders keep their structure; empty folders are `dir` items.
* **Chunk size**: 4 MiB by default (1–32 MiB in settings). A file smaller
  than a chunk is one chunk, so small files are one frame each.
* **Lanes**: the sender opens `min(its setting, the receiver's limit, chunks)`
  data streams (4 by default, up to 16). Each lane has a reader thread
  (disk read + BLAKE3) feeding an async writer (TLS + socket). Lanes take
  the next chunk from one shared queue, so a slow lane never holds up the
  others, and many small files follow each other on a lane without waiting
  for any answer: there is no per-file round trip.
* **Receiver**: each lane has an async reader and a writer thread that hashes
  and writes at the chunk's offset (`pwrite` / `seek_write`), so a slow disk
  never stalls decryption. A file is created as `name.sdpart`, preallocated,
  and renamed when its last chunk is verified; name clashes become
  `name (2).ext`. Paths from the network are sanitised: no `..`, no roots or
  drive letters, no characters Windows refuses, no reserved device names
  (`storage.rs`, `safe_relative`, with tests).
* **Text and clipboard** travel inside the offer; no data stream is opened.
* **Pause/Resume** can come from either side; the sender stops taking new
  chunks, streams stay open.
* **Cancel** from either side ends the transfer and removes partial files.
* A chunk that fails its hash is asked for again with `Retry {item, index}`;
  `ItemError` tells the receiver a source file could not be read.

## 6. Resume

If any stream or the control connection breaks, the receiver keeps every
verified chunk and waits up to 15 minutes. The sender reconnects with
backoff (1, 2, 3, 5, 8, 13, 20, 30 s), sends the same offer with
`resume: true`, and the receiver answers `Accept` with `have`: for each item,
the inclusive ranges of chunk indexes it already holds. Only the rest is
sent. This is tested end to end with a proxy that cuts the connection after
20 MB of a 48 MB file (`crates/core/tests/transfer.rs`).

## 7. Pairing

Pairing turns a nearby device into a trusted one. Trusted devices can send
without a prompt (if the user allows it) and are pinned to their key.

**PIN.** Device A shows six digits for five minutes. On device B the user
types them. Over the TLS connection both run SPAKE2 (Ed25519 group,
symmetric) with the PIN as password, then each proves the shared key with
`BLAKE3-keyed(derive_key(K), own_fp || peer_fp)`. Because each side MACs
the fingerprints it saw in its own TLS handshake, a relay in the middle,
which terminates two different TLS sessions, cannot produce matching MACs.
A wrong PIN can only be tried online, at most five times per PIN.

**QR.** Device A shows
`http://<ip>:<port>/#s=<secret>&i=<id>&k=<fingerprint>&a=<ips>&n=<name>`.
An app that scans it connects, checks that the TLS key matches `k`, and
returns `PairQr {secret}`; only someone who saw A's screen knows the secret.
A phone without the app opens the same URL in its browser (section 8). The
fragment after `#` is never sent over the network by browsers.

Before the first transfer from an unpaired device, both screens can show a
six-digit **safety code** derived from both fingerprints, for users who want
to compare.

## 8. Browser client

The PC serves the same UI over plain HTTP on the same port. Browsers on a
LAN cannot do TLS to a self-signed certificate without a warning page, and
`crypto.subtle` is unavailable outside secure contexts, so this path is not
end-to-end encrypted. It is protected by:

* the 128-bit secret from the QR code (sent as a header, never in a URL),
  valid for 15 minutes, rotated on demand; it opens a session with its own
  random 160-bit token
* 20 failed attempts per minute at most
* the browser client can be turned off in settings

Endpoints (all under `/api/w/`): `hello`, `events` (Server-Sent Events),
`offer`, `PUT up/{transfer}/{item}/{index}` (4 MiB chunks, four at a time,
each retried alone), `text`, `answer`, `cancel`, `dl/{transfer}/{item}`
(Range supported, so Safari can resume) and `zip/{transfer}` (all files as
one zip, built on the fly; CRCs are computed just before each entry so the
headers carry real sizes, which keeps the archive readable by the iOS Files
app; ZIP64 above 4 GiB).

## 9. Ports

| Port | Protocol | Use |
|---|---|---|
| 47470/tcp | TLS 1.3 (`swx/1`) and HTTP | transfers, pairing, browser client |
| 47470/udp | QUIC (`swx/1`) | optional transport |
| 47471/udp | multicast/broadcast | discovery |
| 5353/udp | mDNS | discovery |

The Windows installer adds one inbound firewall rule for `sweaxdrop.exe`
(all profiles) after the user approves the installer's UAC prompt, and
removes it on uninstall. The app never changes firewall settings itself.
