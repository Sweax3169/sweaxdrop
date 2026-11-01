# SweaxDrop prior-art research: wireless file transfer protocols

Compiled 2026-11-01. Scope: how nine existing systems do discovery, transport, encryption/authentication/pairing, resume/integrity, parallelism/chunking, and what throughput they reach. It ends with physical limits, cipher costs, and design implications for SweaxDrop (Windows + Android + iPhone, Rust core).

## How to read this document

Every claim is tagged with one of these markers:

- **[P]** Primary source: protocol spec, source code (file and branch or tag named), peer-reviewed paper, vendor documentation, or a maintainer statement in the project's own tracker.
- **[U]** User report: issue comments, forum posts, press tests. These are uncontrolled, and the conditions are as stated by the reporter.
- **[I]** Inference: my own reasoning or arithmetic from [P]/[U] data. The derivation is shown.

"Not found" means I searched and found no source. It does not mean the feature does not exist. Code was read from the default branch on 2026-11-01 unless a tag is given. MB/s means 10^6 bytes per second unless stated otherwise.

## Overview

| System | Discovery | Transport | Crypto / auth | Resume | Parallelism / chunking | Reported speed (examples) |
|---|---|---|---|---|---|---|
| LocalSend | UDP multicast 224.0.0.167:53317, HTTP subnet scan fallback | HTTP(S) REST, one POST per file | TLS, self-signed RSA-2048; fingerprint pinned from discovery (TOFU) since v1.18; optional 6-digit PIN | No | 1 file at a time (≤v1.15), 2 files (v1.16+); no chunking | 5–20 MB/s with HTTPS (pre-1.18), ~100 MB/s with HTTP; 55 MB/s and 21 MB/s on v1.18.2 [U] |
| KDE Connect | UDP broadcast :1716, plus mDNS `_kdeconnect._udp` | TCP + TLS (TCP client acts as TLS server); payload on separate TCP ports 1739–1764 | Self-signed EC P-256 certs; pairing with 8-hex verification key | No | One TLS connection per file, files in sequence; desktop writes 4 KiB at a time | KB/s to ~5 MB/s; 10–20 Mbit/s [U] |
| Warpinator | Zeroconf `_warpinator._tcp` | gRPC over TLS (:42000); registration on :42001 | Cert encrypted with SecretBox(SHA-256(group code)); default code "Warpinator" | No (restart deletes partial files) | One server stream per operation; 1 MiB (Linux) / 512 KiB (Android) chunks; optional zlib | 11 MB/s = line rate on 100 Mbit; 183 MB/s on 10 GbE; ~5 MB/s to Android [P/U] |
| PairDrop / Snapdrop | WebSocket signaling server; room = public IP | WebRTC data channel (SCTP/DTLS) | DTLS; trust in the signaling server | No | 64,000 B messages, stop-and-wait every 1 MB | 0.3–3 MB/s typical, 15 MB/s best case [U/P] |
| Quick Share | BLE (UUID 0xFE2C) triggers mDNS `_FC9F5ED42C8A._tcp`; QR code | TCP over LAN, upgrade to hotspot / Wi-Fi Direct / WebRTC / BT | UKEY2 (P-256) → AES-256-CBC + HMAC-SHA256; 4-digit PIN | Nearby can send from an offset (on by default); auto-resume flag off by default; app-level use not found | 64 KiB frames; payloads sent one at a time | 30–40 MB/s, peaks 90–110 MB/s; BT fallback 60–80 KB/s [U] |
| AirDrop | BLE adverts with 2-byte contact hashes, then mDNS `_airdrop._tcp` over AWDL | HTTPS over AWDL (POST /Discover, /Ask, /Upload) | TLS; Apple-signed certificates plus "record data"; unauthenticated fallback | Not found | Single upload of a gzip'd cpio archive | AWDL iperf: 758 Mbit/s between Macs [P] |
| Send Anywhere | 6-digit key through vendor server | P2P with "node-server" relay | "SSL"; key valid 10 minutes | Not found | Not found | Not found |
| croc | Relay plus local multicast 239.255.255.250 | TCP via relay ports 9009–9013 (4 data connections); v11 adds userspace WireGuard (Tailcat) | PAKE (P-256) → AES-GCM | Yes (zero-chunk detection) | 32 KiB chunks spread over up to 4 (max 8) connections | LAN up to 100 MB/s; multiplexing −15% time [P/U] |
| magic-wormhole | Rendezvous server + code words | Transit: direct TCP hints or relay :4001 | SPAKE2 → NaCl SecretBox records | No (open since 2016) | Single TCP connection, 16 KiB records | "tens of MB/s" direct, 2–7 MB/s relay [P] |

---

## 1. LocalSend

| Discovery | Transport | Crypto | Resume | Parallelism | Reported speed |
|---|---|---|---|---|---|
| UDP multicast 224.0.0.167:53317; `/register` reply; HTTP subnet-scan "legacy" fallback | REST over HTTP/1.1 (hyper `http1` server), port 53317 | HTTPS with self-signed RSA-2048. Client accepted any cert up to v1.17; v1.18 pins the discovered fingerprint during the handshake. Optional PIN | None. No Range/offset; a failed file is re-sent from byte 0 | ≤v1.15: sequential. v1.16–v1.17: 2 isolates. v1.18: `Pool(2)`. One request body per file | HTTPS 7 vs HTTP 50 MB/s (maintainer, 2023); 15→40 MB/s from Android after the Rust client (maintainer, 2024); v1.18.2: 55 MB/s and 21 MB/s [U] |

### Discovery [P]

- Protocol v2.2 sets defaults of multicast group `224.0.0.167`, UDP port 53317, and HTTP(S) port 53317. The group sits inside 224.0.0.0/24 "because some Android devices reject any other multicast group."
- A device announces a JSON `{alias, version, deviceModel, deviceType, fingerprint, port, protocol, download, announce:true}`. Peers answer with `POST /api/localsend/v2/register` to the announcer, or with a multicast reply (`announce:false`) as fallback.
- "Legacy mode" sends `/register` to every local IP when multicast fails.
- The Rust core (v1.18) repeats each announcement after 100 ms, 500 ms and 2000 ms (`ANNOUNCE_DELAYS` in `packages/core/src/multicast/mod.rs`).

### API and transport [P]

- `POST /prepare-upload` carries metadata only and accepts `?pin=`. The receiver accepts all, some or none of the files and returns a `sessionId` plus one token per accepted file. It answers 204 when nothing is needed.
- `POST /upload?sessionId&fileId&token` carries the raw file as the request body. The spec says "This route can be called in parallel." 422 means a checksum mismatch (added in v2.2).
- `POST /cancel?sessionId`.
- "Download API" / web share: `POST /prepare-download` and `GET /download?sessionId&fileId`, served over plain HTTP "because browsers reject self-signed certificates."
- The v1.18 receiver is a Rust hyper server built with the `http1` feature only. It allows 64 connections in total and 8 per IP (`connection_limit.rs`). It writes through a 512 KiB `BufWriter` fed by a 16-deep channel (`save.rs`).
- There is no chunking in the protocol: each file is one streamed HTTP body.

### Encryption, authentication, PIN [P]

- The fingerprint is the SHA-256 of the certificate in HTTPS mode, and a random string in HTTP mode. Its stated purpose is "to avoid self-discovery and to remember devices."
- Certificates are RSA-2048 with validity from 1975 to 4096 (`packages/core/src/crypto/cert.rs`).
- History of client-side certificate checking:
  - v1.15.0: Dio `client.badCertificateCallback = (cert, host, port) => true` (`app/lib/provider/dio_provider.dart`). Any certificate was accepted.
  - v1.17.0: rhttp `TlsSettings(verifyCertificates: false)` (`app/lib/util/rhttp.dart`).
  - main / v1.18 (commit `3f0df39e8` "verify cert during handshake", 2026-07-27): a `PinnedServerCertVerifier` rejects the handshake when the peer certificate's SHA-256 differs from the expected fingerprint. The comment says "a mismatching peer never receives the request metadata or the file content." With `expected_fingerprint: None` it "accepts any valid certificate. This is trust on first use and is only appropriate for discovery."
- The pinned fingerprint is whatever discovery produced. That is the TLS-observed certificate for HTTP discovery, or the fingerprint claimed in the multicast announcement (`discovery/mod.rs`: "Pin the claimed fingerprint").
- **[I]** Nobody compares fingerprints out of band, so an active attacker present at discovery time can still insert itself. v1.18 does stop a different host from impersonating an already-discovered peer later.
- The server side requests client certificates and "trust[s] any certificate that is valid" (`client_cert_verifier.rs`).
- PIN: the `?pin=` query parameter. The v1.18 server allows 3 failed attempts per IP, then returns 429 (`pin.rs`, `MAX_PIN_ATTEMPTS = 3`).
- Protocol v3 (draft diagrams added 2025-10-31): nonce exchange, signed tokens, explicit `/pair` with public keys, and a WebRTC variant that sends "Binary Data (16 KiB chunks)". `client/v3.rs` and `server/v3.rs` exist in the core. I did not verify whether v3 is on by default in v1.18.

### Sender concurrency, per version [P]

This was read in the Dart and Rust code, not inferred from release notes.

- **≤ v1.15.x:** strictly sequential. `_send()` runs `for (final file in files.values) { … await dio.post(…) }` (`app/lib/provider/network/send_provider.dart` at tag v1.15.0, line ~319). One HTTP request per file, one file in flight.
- **v1.16.0 – v1.17.0 (Nov 2024 – Feb 2025):** `_sendLoop` creates `concurrency` workers over a shared `Queue<SendingFile>`, with `concurrency = uploadIsolateCount`. `_uploadIsolateCount = 2` (`common/lib/src/isolate/parent/parent_isolate_provider.dart` @v1.17.0). So at most 2 files are in flight, each uploaded by the Rust `rhttp` client. The v1.16.0 release note says "LocalSend now uses Rust as HTTP client… The receiver is still written in Dart."
- **v1.18.0+ (Aug 2026):** a single isolate task runs `Pool(_concurrency).forEach(...)` with `const _concurrency = 2` ("How many files of a HttpUploadFilesTask are uploaded in parallel", commit `61cf06bef` "enable concurrency 2 for sending"). One pinned client is "shared by all files of the task so the connection is reused." A file rejected with 422 is retried up to `_maxUploadAttempts = 3` (`packages/localsend_isolates/lib/src/isolate/child/upload_isolate.dart`).

### Integrity [P]

- Since v1.18 the sender hashes every file with SHA-256 **before** `prepare-upload`. In send_provider.dart: "The files are read and hashed in Rust, one file after another." The hash goes into the `sha256` field, and the server verifies it while writing (`save.rs`, `SaveResult::HashMismatch`).
- Hashing is optional (`createChecksums`, enabled by default per the v1.18.0 notes).
- Consequence (issue #3559, v1.18.2) [U]: 8 × 1 GB files took "almost 3 minutes to calculate the checksums… and then only 1 minute for the file transfer itself" (Ryzen 5950X, NVMe SSD).
- **[I]** That is about 45 MB/s of hashing, far below CPU SHA-256 speed (see [Cipher cost](#11-cipher-cost)). The extra full pre-read of every file is the visible cost.

### Resume [P]

- None. There are no `Range`/offset fields in v2.2 (a commenter in #384 noted that "v2 protocol not include `Range`, `Content-Range` supports").
- Retry (`sendFile(isRetry: true)`) re-uploads the whole file.
- A user requested resume in #1313 (500+ files, 20 GB: "restarting an interrupted transfer should resume the unfinished portion").

### iOS specifics [P]

- `app/ios/Runner/Runner.entitlements` contains `com.apple.developer.networking.multicast = true`, so LocalSend obtained Apple's multicast entitlement.
- `Info.plist` declares `NSLocalNetworkUsageDescription` and `NSBonjourServices` (`_http._tcp`, `_bonjour._tcp`, `_lnp._tcp.`).
- The README troubleshooting suggests toggling the Local Network permission on macOS/iOS. It lists a known Android slowness tied to `saf_stream` (Storage Access Framework streams over method channels).

### Reported speeds

| Who / where | Conditions | Number |
|---|---|---|
| Tienisto (maintainer), [#384](https://github.com/localsend/localsend/issues/384), 2023-04-18 [P] | Own test; Android identified as the slow side | HTTP 50 MB/s vs HTTPS 7 MB/s; Windows↔macOS 60 MB/s with HTTPS |
| Tienisto, #384, 2024-01-27 [P] | Explanation | Slowness "caused by the inefficient HTTPS implementation" (Dart); "disable encryption if you want full speed" |
| Tienisto, #384, 2024-11-03 [P] | v1.16.0, Android sender | 40 MB/s, previously 15 MB/s; expects ~60 MB/s once the receiver is in Rust (as demonstrated by `tom8zds/localsend_rs`) |
| MahouShoujoMivutilde, #384, 2024-01-27 [U] | 1 Gbit LAN, Windows notebook → Linux | 21 MB/s vs rsync-over-ssh 91.5 MB/s |
| yotoprules, #384, 2025-12-24 [U] | — | ~6 MB/s with encryption, 103 MB/s without |
| Engino00, #384, 2025-07-25 [U] | 2.5 GbE + 5 GHz, Win↔Android | 10 MB/s default → 100 MB/s with HTTP; Android also copies from a temp dir unless the save path is changed |
| alexdi2, #384, 2026-10-25 [U] | v1.18.2, Windows + iOS, 800 Mbit/s to the router | Max 21 MB/s, often 7–10 MB/s "regardless of encryption"; older versions >100 MB/s with encryption off |
| zamindoost1360, [#2588](https://github.com/localsend/localsend/issues/2588), 2026-09-09 [U] | v1.18.2, laptop → Android, 1.5 GB file | ~55 MB/s |
| LIghtJUNction, [#1090](https://github.com/localsend/localsend/issues/1090), 2025-09-23 [U] | Wi-Fi 6, 5 GHz hotspot | 6 MB/s encrypted → 90 MB/s unencrypted |
| HenryWu01, [#98](https://github.com/localsend/localsend/issues/98), 2023-02-01 [U] | v1.6, Wi-Fi 6, Win11 ↔ Android 13 | 3–4 MB/s vs `python -m http.server` ~35 MB/s; another user measured ~800 Mbit/s iperf3 but 5 MB/s in LocalSend |
| Trackerlist, [#1657](https://github.com/localsend/localsend/issues/1657), 2024-08 [U] | v1.15.3, 7000 images < 1 GB, 2.4 GHz | 1 MB/s → 600 KB/s → 80 KB/s as the file count grew; zip of the same data steady at 4 MB/s. After 1.15.4: "cannot transfer more than two files per second" (2 MB files → 4 MB/s; 10 KB files → 20 KB/s) |
| ProjectInfinity, [#1429](https://github.com/localsend/localsend/issues/1429), 2024-06 [U] | Many small files | "huge amount of overhead between each file"; reporter saw 700 KB/s HTTPS vs 8 MB/s HTTP |
| yankedi, [#2620](https://github.com/localsend/localsend/issues/2620), 2025-08 [U] | Batch of images over Gigabit Ethernet | Does not saturate; a single large file does |
| eugenialoli, [#2547](https://github.com/localsend/localsend/issues/2547), 2025-06 [U] | 10 large + ~3000 small files | ≤3–4 MB/s, degrading over time |

### Sources

- Protocol v2.2: https://github.com/localsend/protocol/blob/main/README.md ; changelog: https://github.com/localsend/protocol/blob/main/CHANGELOG.md ; v3 diagrams: https://github.com/localsend/protocol/blob/main/v3/http-diagram.mermaid , https://github.com/localsend/protocol/blob/main/v3/webrtc-diagram.mermaid
- Sender (main): https://github.com/localsend/localsend/blob/main/packages/localsend_isolates/lib/src/isolate/child/upload_isolate.dart , https://github.com/localsend/localsend/blob/main/packages/localsend_isolates/lib/src/task/upload/http_upload.dart , https://github.com/localsend/localsend/blob/main/app/lib/provider/network/send_provider.dart
- Sender (old): https://github.com/localsend/localsend/blob/v1.15.0/app/lib/provider/network/send_provider.dart , https://github.com/localsend/localsend/blob/v1.17.0/app/lib/provider/network/send_provider.dart , https://github.com/localsend/localsend/blob/v1.17.0/common/lib/src/isolate/parent/parent_isolate_provider.dart
- TLS: https://github.com/localsend/localsend/blob/v1.15.0/app/lib/provider/dio_provider.dart , https://github.com/localsend/localsend/blob/v1.17.0/app/lib/util/rhttp.dart , https://github.com/localsend/localsend/blob/main/packages/core/src/http/client/server_cert_verifier.rs , https://github.com/localsend/localsend/blob/main/packages/core/src/http/server/common/client_cert_verifier.rs , https://github.com/localsend/localsend/blob/main/packages/core/src/crypto/cert.rs , https://github.com/localsend/localsend/blob/main/packages/core/src/discovery/mod.rs
- Server: https://github.com/localsend/localsend/blob/main/packages/core/src/http/server/common/save.rs , https://github.com/localsend/localsend/blob/main/packages/core/src/http/server/v2.rs , https://github.com/localsend/localsend/blob/main/packages/core/src/http/server/common/pin.rs , https://github.com/localsend/localsend/blob/main/packages/core/src/http/server/connection_limit.rs , https://github.com/localsend/localsend/blob/main/packages/core/Cargo.toml , https://github.com/localsend/localsend/blob/main/packages/core/src/multicast/mod.rs
- iOS: https://github.com/localsend/localsend/blob/main/app/ios/Runner/Runner.entitlements , https://github.com/localsend/localsend/blob/main/app/ios/Runner/Info.plist
- Releases: https://github.com/localsend/localsend/releases/tag/v1.16.0 , https://github.com/localsend/localsend/releases/tag/v1.18.0 ; README troubleshooting: https://github.com/localsend/localsend#troubleshooting
- Issues: #98, #384, #1090, #1313, #1429, #1657, #2547, #2588, #2620, #3559 at https://github.com/localsend/localsend/issues/<number>

---

## 2. KDE Connect

| Discovery | Transport | Crypto | Resume | Parallelism | Reported speed |
|---|---|---|---|---|---|
| UDP broadcast of the identity packet to port 1716; mDNS `_kdeconnect._udp.local` | TCP (link port 1716–1764) + TLS; payloads on a new TCP+TLS connection on 1739–1764 | Self-signed certs (new keys EC P-256); unpaired = accept any cert; paired = pinned cert; 8-hex verification key | None found | One payload connection per file, files in sequence (next file announced while the current one is sent) | KB/s – 5 MB/s; 10–20 Mbit/s [U] |

### Discovery [P]

- `LanLinkProvider` binds UDP 1716 and sends the identity packet (with its `tcpPort`) to the broadcast address and from every local IP.
- The receiver of a broadcast opens a TCP connection to the sender's `tcpPort` (range 1716–1764) (`core/backends/lan/lanlinkprovider.{h,cpp}`).
- mDNS discovery uses service type `_kdeconnect._udp.local` (`mdnshdiscovery.cpp`), through Avahi or the bundled mdns.h.
- A Bluetooth backend also exists (`core/backends/bluetooth`).
- Protocol version 8 is current (`NetworkPacket::s_protocolVersion = 8`). Devices with version < 8 are ignored, and a trusted device downgrading its version is rejected.

### TLS and pairing [P]

- Role reversal: the side that opened the TCP connection sends its identity in plaintext and then calls `startServerEncryption()`. The log line reads "Starting server SSL (I'm the client TCP socket)".
- For an untrusted device, `QSslSocket::QueryPeer` is used and `SelfSignedCertificate` errors are ignored. For a trusted device, the stored certificate is the only CA with `VerifyPeer`. `setPeerVerifyName(deviceId)` binds the certificate name to the device id.
- After TLS, the identity packet is sent again inside the tunnel ("secure identity"). A protocol-version change between the two is rejected.
- Keys: `generateEcPrivateKey()` on `NID_X9_62_prime256v1`, with an RSA path for legacy. Validity is from −1 year to +10 years (`sslhelper.cpp`).
- Pairing: a `kdeconnect.pair` packet carries a `timestamp`, rejected if it is more than 1800 s off. Both sides display a **verification key**: the first 8 hex characters (uppercase) of SHA-256 over the two certificate public keys (DER, sorted) followed by the timestamp (`core/backends/pairinghandler.cpp`).
- **[I]** This gives users a real out-of-band check of 32 bits.
- The Android client pins `SSLContext.getInstance("TLSv1.2")`, "since 1.3 seems to cause issues in some (older?) devices", and uses a trust-all manager for unpaired devices (`helpers/security/SslHelper.kt`).

### Payload transfer [P]

- The sender listens on a port in 1739–1764 and announces it in `payloadTransferInfo: {port}`. The receiver connects, and the sender again becomes the TLS server, verifying the trusted certificate (`compositeuploadjob.{h,cpp}`).
- Each file uses its own connection. The next file is announced while the current one is still being sent, and its connection is left pending.
- Desktop `UploadJob::uploadNextPacket()` reads at most **4096 bytes** and writes them. It only reads the next 4 KiB when `encryptedBytesToWrite() == 0` (`uploadjob.cpp`).
- **[I]** That is a user-space stop-and-wait at 4 KiB granularity. The kernel send buffer hides part of it, but each refill waits for Qt's TLS buffer to drain.
- Android uses 256 KiB copy buffers and a 1 MiB `BufferedOutputStream` on receive (`LanLink.kt`, `CompositeReceiveFileJob.kt`).
- I found no checksum or resume code in the share plugin or jobs.

### Reported speeds [U]

- Bug [396627](https://bugs.kde.org/show_bug.cgi?id=396627) (Android → PC, since 2018, status CONFIRMED): "several KB/s to 3.1 MB/s" from a Galaxy S8 and up to 5 MB/s from a Nexus 5, with router link speeds above 780 / 380 Mbit/s. The bug has no developer root-cause analysis.
- [discuss.kde.org 28905](https://discuss.kde.org/t/why-is-kde-connect-not-as-fast-as-syncthing/28905): 10–20 Mbit/s for multiple files vs Syncthing 100+ Mbit/s on the same 6 GHz Wi-Fi. No developer replied.
- No measured number from the developers was found.

### Sources

- https://github.com/KDE/kdeconnect-kde/blob/master/core/backends/lan/lanlinkprovider.h , …/lanlinkprovider.cpp , …/compositeuploadjob.h , …/compositeuploadjob.cpp , …/uploadjob.cpp , …/mdnshdiscovery.cpp (all under https://github.com/KDE/kdeconnect-kde/tree/master/core/backends/lan)
- https://github.com/KDE/kdeconnect-kde/blob/master/core/backends/pairinghandler.cpp , https://github.com/KDE/kdeconnect-kde/blob/master/core/sslhelper.cpp , https://github.com/KDE/kdeconnect-kde/blob/master/core/networkpacket.cpp
- Android: https://github.com/KDE/kdeconnect-android/blob/master/src/main/java/org/kde/kdeconnect/helpers/security/SslHelper.kt , …/backends/lan/LanLink.kt , …/plugins/share/CompositeReceiveFileJob.kt
- https://bugs.kde.org/show_bug.cgi?id=396627 ; https://discuss.kde.org/t/why-is-kde-connect-not-as-fast-as-syncthing/28905

---

## 3. Warpinator (linuxmint/warpinator and the Android port)

| Discovery | Transport | Crypto | Resume | Parallelism | Reported speed |
|---|---|---|---|---|---|
| Zeroconf `_warpinator._tcp.local.` | gRPC over TLS, port 42000; registration port 42001 | Self-signed RSA-2048 (30-day validity); cert exchanged encrypted with NaCl SecretBox keyed by SHA-256(group code); default code "Warpinator" | None; a restart deletes partial files | One server-streaming RPC per operation; files in sequence; 1 MiB (Linux) / 512 KiB (Android) chunks; optional zlib | 11 MB/s on a ~94 Mbit/s link (= line rate); 183 MB/s on 10 GbE; ~5 MB/s on Android paths [P/U] |

### Details [P]

- **Discovery:** `SERVICE_TYPE = "_warpinator._tcp.local."` with TXT properties such as `hostname` (`src/server.py`).
- **Registration and auth:**
  - API v2 uses an insecure gRPC channel to the registration port (default 42001) calling `WarpRegistration.RequestCertificate` (`remote_registration.py`, `warp.proto`). API v1 requested the certificate over UDP (`register_v1`; the Android port's `CertServer`).
  - The returned certificate is encrypted with `nacl.secret.SecretBox(SHA-256(group_code))`. The receiver can decrypt it only if it knows the same group code (`src/auth.py`).
  - The default group code is the string `'Warpinator'` (`data/org.x.Warpinator.gschema.xml`).
  - Certificates are RSA-2048, `EXPIRE_TIME = 30 days`, with the IP address as a critical SAN.
  - **[I]** With the default group code, any Warpinator instance on the LAN is trusted. A sniffed encrypted cert also lets an attacker guess a non-default group code offline.
- **Transport:**
  - `grpc.secure_channel(ip:port, grpc.ssl_channel_credentials(cert))`: the peer's certificate is the root of trust (`remote.py`).
  - The receiver calls `StartTransfer(OpInfo) returns (stream FileChunk)`, so the sender streams one ordered sequence of `FileChunk{relative_path, chunk, …}` messages for all files in the operation (`warp.proto`, `transfers.py`).
  - Block size is `transfer-block-size` × 1024, default 1024 KiB ("Max is 4094"). The Android port uses `CHUNK_SIZE = 512 kB` (`Transfer.java`).
- **Compression:** `use-compression` defaults to false, with zlib level −1 (= 6). It is applied by gRPC interceptors only "if both ends support it" (gschema, `server.py`, `remote.py`).
- **Resume/integrity:** re-requesting an operation re-enters `WAITING_PERMISSION`. The receiver runs `clean_existing_files()` ("Removing any existing files matching the pending transfer") (`server.py`, `transfers.py`). No file hash is computed; integrity relies on TLS.

### Reported speeds

- mtwebster (maintainer), [#68](https://github.com/linuxmint/warpinator/issues/68), 2021 [P]: Warpinator 11 MB/s vs iperf 94.2 Mbit/s (11.7 MB/s) vs FTP 11 MB/s; "250 MB/s+ transferring to a hosted virtual machine – the program itself isn't slowing things down."
- melroy89, [#224](https://github.com/linuxmint/warpinator/issues/224), 2025 [U]: v1.8.8, 10 GbE, compression off, 4 GB file. 183 MB/s vs iperf3 9.2 Gbit/s and FileZilla (SFTP) 415 MB/s. CPU saturation not yet checked.
- slowscript (Android maintainer), [warpinator-android #75](https://github.com/slowscript/warpinator-android/issues/75), 2021 [P]: 4.9 MB/s vs 6.0 MB/s for another app on 2.4 GHz. He suspects "overhead of the protocol we are using (gRPC)".
- Same thread [U]: Windows → Pixel 6a 5 MB/s, but Pixel 6a → Windows 30–40 MB/s.

### Sources

- https://github.com/linuxmint/warpinator/blob/master/src/auth.py , …/src/transfers.py , …/src/server.py , …/src/remote.py , …/src/remote_registration.py , …/src/warp.proto , …/src/prefs.py , https://github.com/linuxmint/warpinator/blob/master/data/org.x.Warpinator.gschema.xml
- https://github.com/slowscript/warpinator-android/blob/master/app/src/main/java/slowscript/warpinator/Transfer.java , …/CertServer.java
- Issues: https://github.com/linuxmint/warpinator/issues/68 , https://github.com/linuxmint/warpinator/issues/224 , https://github.com/slowscript/warpinator-android/issues/75

---

## 4. PairDrop / Snapdrop

| Discovery | Transport | Crypto | Resume | Parallelism | Reported speed |
|---|---|---|---|---|---|
| WebSocket signaling server; room key = client's public IP (private IPs collapse to one room); PairDrop adds paired rooms (secret) and public room IDs | WebRTC data channel (ordered); optional TURN; optional WebSocket relay fallback | WebRTC DTLS; signaling server trusted for SDP | None found | Files one after another; 64,000-byte messages; stop-and-wait ack every 1 MB | 0.3–3 MB/s typical [U]; 15 MB/s after the chunker rewrite (maintainer) |

### Details [P]

- **Rooms by IP:**
  - PairDrop `server/peer.js` takes `cf-connecting-ip`, else the first `x-forwarded-for`, else `socket.remoteAddress`.
  - IPv6 can be truncated to a prefix (`ipv6Localize`). Private and loopback addresses are rewritten to `127.0.0.1`, so all private-address clients of a self-hosted server share one IP room.
  - Snapdrop's `server/index.js` keys `_rooms[peer.ip]`.
- **Signaling and ICE:**
  - Snapdrop's client hard-codes `stun:stun.l.google.com:19302`.
  - PairDrop reads `RTC_CONFIG` (STUN/TURN JSON, e.g. `turns:<DOMAIN>:5349`) and supports `WS_FALLBACK` (data relayed through the server; default false). The docs say "you have to host your own TURN server to enable transfers between different networks."
- **Channel:** `createDataChannel('data-channel', { ordered: true, … })`.
- **Chunking (master and `next` branches of PairDrop, and Snapdrop):**
  - `FileChunker` uses `_chunkSize = 64000` and `_maxPartitionSize = 1e6`. After each partition the sender sends `{type:'partition', offset}` and waits for the receiver's `partition-received` before reading the next partition.
  - **[I]** At most ~1 MB is in flight per RTT of the ack path, so throughput ≤ 1 MB / RTT_ack. Any RTT stall caps throughput no matter the link speed.
- **Unreleased branch `speed-up-webrtc`:** chunk = `min(sctp.maxMessageSize, 262144)`, `bufferedAmountLowThreshold = 2 × chunk`, high-water mark 8 × chunk. This is proper flow control.
- **Integrity/resume:** no file hash or resume found in `network.js`.

### Browser SCTP limits

- [P] Mozilla bug 1051685: Firefox data channels used a fixed 128 KiB SCTP window. Michael Tüxen notes throughput is bounded by min(send window, peer rwnd)/RTT. The window was raised to 1 MiB, FIXED in Firefox 64.
  - Lennart Grahl's same-machine numbers: Firefox 57 ↔ 57 ~93 Mbit/s, RAWRTC ↔ RAWRTC ~310–422 Mbit/s, FF58 (1 MiB) ~176 Mbit/s. He reports flaky results.
- [P] Axis Communications (Emil Ljungdahl), GStreamer Conference, 24 Oct 2025, usrsctp-based server sending 16 KiB messages:
  - Local network: ~100 Mbit/s vs TCP ~300 Mbit/s before fixing a 100 ms sleep in `gstsctpenc`.
  - 80 ms RTT: Chrome ~8 Mbit/s, Firefox ~12–55 Mbit/s, vs TCP ~90. With SACK-IMMEDIATELY: Chrome ~15, Firefox ~55 Mbit/s.
  - 80 ms with reordering: ~600 kbit/s, ~6 Mbit/s with experimental RACK.
- [P] MDN: if `max-message-size` is absent from SDP, 64 KiB is assumed. "most modern browsers support sending messages of at least 256 kilobytes". Large messages cause head-of-line blocking without RFC 8260 interleaving.

### Reported speeds

- schlagmichdoch (maintainer), [#44](https://github.com/schlagmichdoch/PairDrop/issues/44), 2024-07-17 [P]: after rewriting chunker/digester (dev instance), "a 1.5GB file from Ubuntu Firefox to iOS Safari on the same network with speeds up to 15 MB/s". Also: "we are bound by the abilities of browsers as their WebRTC implementation is the current bottleneck."
- Same thread [U]: 300–400 KB/s (2023-09); "about 2M/s even 500k/s" on a Wi-Fi 6 5 GHz hotspot (2023-05); 260 MB in ~10 min on "Gigabit" Wi-Fi (2024-05).
- [#359](https://github.com/schlagmichdoch/PairDrop/issues/359) (2025) [P]: Android → Windows "significantly slower". The maintainer says "the code to chunk a file and send it is pretty slow and its speed depends on the device, the browser, and whether the device is in energy save mode."

### Sources

- https://github.com/schlagmichdoch/PairDrop/blob/master/public/scripts/network.js , https://github.com/schlagmichdoch/PairDrop/blob/master/server/peer.js , https://github.com/schlagmichdoch/PairDrop/blob/master/server/ws-server.js , https://github.com/schlagmichdoch/PairDrop/blob/master/rtc_config_example.json , https://github.com/schlagmichdoch/PairDrop/blob/master/docs/host-your-own.md , https://github.com/schlagmichdoch/PairDrop/blob/speed-up-webrtc/public/scripts/network.js
- https://github.com/RobinLinus/snapdrop/blob/master/client/scripts/network.js , https://github.com/RobinLinus/snapdrop/blob/master/server/index.js
- https://bugzilla.mozilla.org/show_bug.cgi?id=1051685 ; https://indico.freedesktop.org/event/11/contributions/461/attachments/320/418/datachannel_optimizations_gstreamer_conf.pdf ; https://developer.mozilla.org/en-US/docs/Web/API/WebRTC_API/Using_data_channels
- Issues: https://github.com/schlagmichdoch/PairDrop/issues/44 , /359 , /244

---

## 5. Quick Share (formerly Nearby Share; Android, ChromeOS, Windows)

| Discovery | Transport | Crypto | Resume | Parallelism | Reported speed |
|---|---|---|---|---|---|
| BLE advert (service UUID 0xFE2C) makes Android advertise mDNS `_FC9F5ED42C8A._tcp`; QR-code mode | Length-prefixed protobuf over TCP on Wi-Fi LAN; "bandwidth upgrade" to Wi-Fi hotspot / Wi-Fi Direct / WebRTC / BT / AWDL / Wi-Fi Aware | UKEY2 (P-256, SHA-512 commitment) → HKDF → AES-256-CBC + HMAC-SHA256 per frame, with sequence numbers; 4-digit PIN | Sending from a non-zero offset is enabled by default in open-source Nearby; "auto-resume" after disconnect defaults to off; Quick Share's app-level use not found | Payloads of a type sent one at a time (FCFS); 64 KiB default frame | 30–40 MB/s, peaks 90–110 MB/s (Android → Windows); BT 60–80 KB/s [U] |

### Discovery [P] (NearDrop PROTOCOL.md, reverse-engineered by grishka)

- The receiver ("server") advertises mDNS type `_FC9F5ED42C8A._tcp.`, where FC9F5E… comes from SHA-256("NearbySharing").
  - Name: URL-safe base64 of `0x23 | 4-byte endpoint ID | FC 9F 5E | 00 00`.
  - TXT `n`: endpoint info (version/visibility/device type bits, 16 bytes of salt + encrypted metadata key, device name, optional TLVs such as QR data or vendor ID).
- Android only advertises after it sees a BLE advertisement with service UUID `fe 2c` and service data `fc 12 8e 01 42 00 …`. NearDrop notes macOS cannot emit that service data.
- QR mode: `https://quickshare.google/qrcode#key=…`. The advertising token and name key are derived with HKDF-SHA256. To skip the prompt, the sender signs the UKEY2 auth key with the QR key (ECDSA, P1363 format).

### Handshake and encryption [P]

- Each message carries a 4-byte big-endian length prefix.
- Sequence: ConnectionRequest → UKEY2 ClientInit / ServerInit / ClientFinish → ConnectionResponse.
- The next-protocol secret goes through HKDF to D2D client/server keys (salt SHA-256("D2D")), then to the encryption and HMAC keys (salt SHA-256("SecureMessage"), info `ENC:2` / `SIG:1`).
- After that, every frame is a `SecureMessage`: AES-256-CBC (PKCS7, random 16-byte IV) plus HMAC-SHA256, wrapping a `DeviceToDeviceMessage` with a per-direction sequence number.
- The UKEY2 authentication string becomes a **4-digit PIN** for visual comparison.
- NearDrop's Swift code uses `handshakeCipher = .p256Sha512`.
- Keep-alive frames are needed every 10 s.

### Payloads and bandwidth upgrade [P]

- Payload frames carry `id`, `type`, `totalSize` and chunk `{offset, flags(LAST_CHUNK), body}`. The id "allows transferring multiple payloads in parallel" at the protocol level.
- Google's open-source Nearby Connections (`google/nearby`):
  - `PayloadManager` sends "each payload … in FCFS order within each Payload type, blocking any other payload of the same type from even starting until this one is completely done" on a `SingleThreadExecutor`.
  - Chunk size = `GetMaxTransmitPacketSize()`, which defaults to the flag `kMediumDefaultMaxTransmitPacketSize = 65536`.
  - A `resume_offset` path exists, gated by `enable_send_payload_offset` ("Support sending file and stream payloads starting from a non-zero offset"), which defaults to `true` in `internal/platform/feature_flags.h`. `kSafeToDisconnectVersion` (values for "auto-resume", "payload_ack") defaults to 0.
  - I did not find whether the Quick Share application layer (`sharing/`) uses offsets to resume interrupted transfers.
- NearDrop itself sends 512 KiB chunks.
- `BwuManager` registers upgrade handlers for AWDL, WIFI_HOTSPOT, WIFI_DIRECT, WIFI_LAN, WIFI_AWARE_R4, WEB_RTC and BLUETOOTH. The repository has Windows implementations for `wifi_hotspot_medium`, `wifi_direct_medium`, `wifi_lan_mdns`, BLE and Bluetooth Classic.
- **[I]** The shipping Windows app very likely uses this codebase, but I did not verify that.
- Google's developer docs describe the API as using "a combination of Bluetooth, BLE, and Wi-Fi technologies". Point-to-point strategy offers "the highest possible throughput".
- **[I]** AES-CBC encryption is serial within a message, and CBC+HMAC means two passes over the data. At 64 KiB frames that is acceptable but not free.

### AirDrop interoperability (2025) [P]

Google announced Quick Share ↔ AirDrop on Pixel 10 in November 2025 (security blog URL below). Per The Hacker News summary:

- The Apple device must be in "Everyone for 10 minutes" mode.
- The connection is direct and peer-to-peer, not routed through a server.
- The implementation is in Rust.
- NetSPI assessed it in August 2025 and found one low-severity issue, since fixed.

### Reported speeds [U]

Mishaal Rahman, Android Authority, 2024-10-27:

- 5.4 GB file over Bluetooth ~60 KB/s; over Wi-Fi ~30 MB/s.
- With the PC's Wi-Fi on but not connected to any network, phones started at 60–80 KB/s (BT) and then rose to 30–40 MB/s after a "DIRECT-XXXXX" hotspot appeared.
- Xiaomi 14T Pro → Galaxy Book5 Pro 360 peaked at 90–110 MB/s, usually 30–40 MB/s.

### Sources

- https://github.com/grishka/NearDrop/blob/master/PROTOCOL.md , https://github.com/grishka/NearDrop/blob/master/README.md , https://github.com/grishka/NearDrop/blob/master/NearbyShare/OutboundNearbyConnection.swift
- https://github.com/google/nearby/blob/main/connections/implementation/payload_manager.cc , https://github.com/google/nearby/blob/main/internal/platform/feature_flags.h , https://github.com/google/nearby/blob/main/connections/implementation/base_endpoint_channel.cc , https://github.com/google/nearby/blob/main/connections/implementation/bwu_manager.cc , https://github.com/google/nearby/blob/main/connections/implementation/flags/nearby_connections_feature_flags.h , https://github.com/google/nearby/tree/main/internal/platform/implementation/windows
- https://developers.google.com/nearby/connections/overview , https://developers.google.com/nearby/connections/strategies
- https://www.androidauthority.com/quick-share-for-windows-wifi-direct-3494451/
- https://security.googleblog.com/2025/11/android-quick-share-support-for-airdrop-security.html , https://thehackernews.com/2025/11/google-adds-airdrop-compatibility-to.html

---

## 6. AirDrop

| Discovery | Transport | Crypto | Resume | Parallelism | Reported speed |
|---|---|---|---|---|---|
| BLE non-connectable adverts with first 2 bytes of SHA-256 of up to 4 contact identifiers; then mDNS `_airdrop._tcp` on the AWDL interface (port 8770 in the paper's example) | HTTPS over AWDL (IPv6 link-local): POST /Discover, /Ask, /Upload | TLS with client and server certs; Apple-signed per-device cert + Apple-signed "record data" (UUID + SHA-256 of contact identifiers); silent fallback to unauthenticated | Not found | One /Upload of a gzip-compressed cpio archive per transfer | AWDL TCP 758 Mbit/s between Macs (Stute et al. 2018) [P] |

### Details [P]

- **AWDL:** Stute, Kreitschmann, Hollick (MobiCom 2018) reverse-engineered it.
  - Nodes announce Availability Windows synchronized by an elected master. Outside these windows the radio can serve an AP channel.
  - With traffic, AWDL allocates more of the channel sequence to itself. At least 25% is reserved for the AP when the device is associated, and there is a forced switch to channel 6 in slot 9.
  - Apple drops packets for ordinary TCP/UDP servers bound to `awdl0`. The authors needed an NSNetService-based proxy to run iperf.
- **AirDrop flow:** Stute et al. (USENIX Security 2019).
  - The sender emits BLE adverts (Apple manufacturer data 0x4c00, subtype 0x05). They contain "the first two bytes of the SHA2 digest of the sender's contact identifiers."
  - The receiver checks them against its address book in Contacts-only mode, activates AWDL, and is found via mDNS.
  - HTTPS `/Discover` exchanges record data, `/Ask` triggers the accept prompt, and `/Upload` carries the data.
  - "AirDrop transparently treats a connection as unauthenticated if the sender or receiver fails to provide an Apple-signed TLS certificate or valid record data." The paper shows how to downgrade and MitM the connection.
  - The 2-byte hashes make brute-force activation of AWDL feasible with a $20 micro:bit.
- **Apple Platform Security:** devices "use Bluetooth Low Energy (BLE) and Apple-created peer-to-peer Wi-Fi technology". Signing in to iCloud creates a "2048-bit RSA identity", and the peer-to-peer Wi-Fi connection "is encrypted with TLS". It compares short and long identity hashes, with Contacts Only as the default.
- **OpenDrop** (TU Darmstadt, Python):
  - Browses `_airdrop._tcp.local.` and POSTs `/Discover`, `/Ask`, then `/Upload` with `Content-Type: application/x-cpio` from a gzip-filtered cpio archive.
  - Its README states it does not verify Apple-root-signed TLS certificates or validation records, and sends only one file per archive.
  - OWL (the Linux AWDL implementation) needs a Wi-Fi card with active monitor mode and frame injection, and cannot run alongside an AP connection.

### Measured AWDL throughput [P]

Stute et al., MobiCom 2018, Fig. 15: iperf TCP, MacBook ↔ iMac, 50 × 10 s runs. Values were read from the figure's bar labels.

| Setting | Throughput |
|---|---|
| Only AWDL | ~758 Mbit/s |
| Only AP (iMac as AP, 3-stream capable) | ~773 Mbit/s |
| Only AP (APU, 2-stream Qualcomm, 866.7 Mbit/s PHY) | ~546 Mbit/s |
| IBSS | ~685 Mbit/s |
| AWDL + AP connection, same channel | ~270 + ~259 Mbit/s (total ~529) |
| AWDL + AP connection, different channels | ~300 + ~159 Mbit/s; total "drops by about 13%" |

The authors' conclusion: throughput "is only limited by the devices' supported PHY data rates if the nodes are not actively using an infrastructure network." I found no peer-reviewed iPhone-to-iPhone AirDrop throughput figure.

### Sources

- https://arxiv.org/abs/1808.03156 (MobiCom'18, "One Billion Apples' Secret Sauce")
- https://www.usenix.org/system/files/sec19-stute.pdf (USENIX Security'19, "A Billion Open Interfaces for Eve and Mallory")
- https://support.apple.com/guide/security/airdrop-security-sec2261183f4/web
- https://github.com/seemoo-lab/opendrop (README, `opendrop/client.py`) ; https://github.com/seemoo-lab/owl

---

## 7. Send Anywhere (Estmob)

| Discovery | Transport | Crypto | Resume | Parallelism | Reported speed |
|---|---|---|---|---|---|
| 6-digit one-time key (or QR) brokered by the vendor's server | "p2p networking"; if P2P fails, "the most effective node-server to pass through without storing any files" | "SSL encryption on our website… and mobile apps"; key details not documented | Not found | Not found | Not found |

### Details

- No protocol specification or source code was found.
- The only vendor-authored description I found is Estmob's WordPress plugin text, now closed (closure notice dated 5 Feb 2026), via a mirror [P, vendor marketing]:
  - "a one time six digit code that will sync the sending and receiving device"
  - "SA utilizes p2p networking to bypass cloud storage"
  - "If the devices are in remote locations or in different networks, the p2p path may sometimes fail", in which case a node server relays "without storing any files"
  - recipients must download "within a ten minute time frame"
  - a separate upload mode keeps files up to 24 hours
- Secondary sites mention "256-bit encryption" without a vendor source. I treat that as unverified.

### Sources

- https://fair.pm/packages/plugins/send-anywhere-file-transfer/ (mirror of the Estmob plugin text) ; https://wordpress.org/plugins/send-anywhere-file-transfer/ (closure notice)

---

## 8. croc (schollz/croc, v11.6.0, Oct 2026)

| Discovery | Transport | Crypto | Resume | Parallelism | Reported speed |
|---|---|---|---|---|---|
| Code phrase → room on a relay (default pool incl. `croc.schollz.com:9009`); sender also runs a local relay and multicasts on 239.255.255.250 | TCP through relay ports (9009 control, 9010–9013 data). v11 `--transport auto`: Tailscale userspace WireGuard ("Tailcat") via DERP, upgraded to direct UDP when NAT traversal works | PAKE (schollz/pake v3, PAKE2 / SPAKE2-style, default curve P-256), identity-bound; data in AES-GCM | Yes: receiver detects missing 32 KiB chunks in a pre-sized file | 32 KiB chunks of one file spread over up to 4 data connections (max 8); files in sequence | LAN "100 MB/s, the max on my connections" (maintainer); multiplexing −15% time [P/U] |

### Details [P]

- **Relay:**
  - Defaults `DEFAULT_RELAY = "croc.schollz.com:9009"`, ports `9009,9010,9011,9012,9013`, relay password `pass123` (`src/models/constants.go`, `src/cli/cli.go`).
  - In v11 the README says public transfers pick one of three relays by SHA-256(code) mod 3. A relay needs "at least 2" ports and admits 64 incomplete handshakes per port, 32 per IP.
- **Local:**
  - Unless `--no-local`, the sender starts a local relay and broadcasts `"croc"+port` with `schollz/peerdiscovery` (`--multicast` default `239.255.255.250`). The receiver probes multicast for 200–500 ms.
  - `--local` forces LAN only.
- **PAKE:**
  - `pakekey.Init` binds identities (`croc/spake2/participant/v2`) and derives traffic keys with HKDF.
  - The pake library says it is "derived from Dan Boneh and Victor Shoup's cryptography book (pg 789, 'PAKE2 protocol')". The curve is selectable (`--curve`, default `p256`).
  - Data uses `crypt.NewAESGCM(c.Key)`.
- **v11 Tailcat:** "two compatible native clients create PAKE-bound Tailcat node identities and open one or more TCP streams over an in-process Tailscale userspace WireGuard network. Magicsock starts through DERP and promotes the connection to a direct UDP path whenever NAT traversal succeeds." It falls back to croc relay ports for browsers, older clients and failures.
- **Chunking/parallelism:**
  - `TCP_BUFFER_SIZE = 64 KiB`, chunk = `TCP_BUFFER_SIZE/2` = 32 KiB, with an 8-byte offset header per chunk.
  - `requestedChunkQueue` "assigns every requested file offset to exactly one worker" across `min(len(RelayPorts), 8)` data connections. `--no-multi` disables this (`src/croc/chunk_queue.go`, `croc.go`).
- **Compression:** flate in Huffman-only mode per chunk. Each file's first block is sampled, and if deflate saves less than 2% the file is sent uncompressed (`shouldCompressFile`). `--no-compress` turns it off.
- **Integrity:**
  - The sender hashes each file before sending (default `xxhash`, i.e. XXH64 over the full file; options `imohash`, `md5`, `highway`). v11 adds negotiated progressive `imohash-v2`. The receiver compares after writing.
  - imohash "uses file size and sampling", so it does not cover every byte.
- **Resume:**
  - The receiver pre-sizes the output file (`Truncate(size)`).
  - On reconnect, if a file of the right size exists, `utils.MissingChunks` scans it in 32 KiB blocks and requests only the **all-zero** blocks.
  - **[I]** This needs no state file. Chunks that are truly zero are simply resent, and the final hash catches anything else.

### Measurements

- schollz, [#91](https://github.com/schollz/croc/issues/91), 2018 [P]: 100 MB random file on a LAN, scp 12.4 s vs croc 14 s ("16% slower"). He explains relay throughput as the harmonic mean of the upload and download links.
- schollz, [#447](https://github.com/schollz/croc/issues/447), 2021 [P]: "When I do LAN croc I get maximum speed transfers (100 MB/s, the max on my connections)." The reporter saw < 1 MB/s vs scp 10 MB/s.
- ferebee, [#602](https://github.com/schollz/croc/issues/602), 2023-10 [U]: 670 MB file, macOS ↔ macOS, 100 Mbit/s DSL down, 6 runs. Multiplexed average 78 s vs `--no-multi` 92 s; with compression off, 76 s vs 111 s.
- schollz in the same thread [P]: without encryption, 105 s multiplexed vs 115 s single.
- T3rm1, [#584](https://github.com/schollz/croc/issues/584), 2023 [U]: 16 files totalling 2.4 MB took > 10 s; `--zip` was "super fast". This is per-file overhead.
- schollz, [#1222](https://github.com/schollz/croc/issues/1222), 2026 [P]: disabling encryption gives an improvement "so marginal it's not worth it."

### Sources

- https://github.com/schollz/croc/blob/main/README.md , https://github.com/schollz/croc/blob/main/src/models/constants.go , https://github.com/schollz/croc/blob/main/src/croc/croc.go , https://github.com/schollz/croc/blob/main/src/croc/chunk_queue.go , https://github.com/schollz/croc/blob/main/src/utils/utils.go , https://github.com/schollz/croc/blob/main/src/compress/compress.go , https://github.com/schollz/croc/blob/main/src/crypt/crypt.go , https://github.com/schollz/croc/blob/main/src/pakekey/pakekey.go , https://github.com/schollz/croc/blob/main/src/croc/progressive_hash.go , https://github.com/schollz/croc/blob/main/src/cli/cli.go , https://github.com/schollz/croc/blob/main/internal/tailcat/tailcat.go
- https://github.com/schollz/pake , https://github.com/schollz/peerdiscovery , https://github.com/kalafut/imohash
- Issues: https://github.com/schollz/croc/issues/91 , /447 , /584 , /602 , /635 , /1222

---

## 9. magic-wormhole

| Discovery | Transport | Crypto | Resume | Parallelism | Reported speed |
|---|---|---|---|---|---|
| Mailbox/rendezvous server (`wss://relay.magic-wormhole.io/v1`); nameplate + code words | "Transit": direct TCP hints (all local IPs) raced with relay `tcp:transit.magic-wormhole.io:4001` (TCP or WebSocket); Tor hints | SPAKE2 → HKDF → NaCl SecretBox for mailbox messages and for every transit record; optional verifier string | No (issue #88 open since 2016) | One TCP connection; Python CLI writes 16 KiB records; directories zipped first | "tens of MBps" direct, 20–50 Mbit/s typical via relay (maintainer, 2020) |

### Details [P]

- **PAKE:** clients send `pake_v1` (SPAKE2 `X+M*pw` / `Y+N*pw`). Then all mailbox messages are SecretBox-encrypted with keys from HKDF-SHA256 over the shared key and `wormhole:phase:{sha256(side)}{sha256(phase)}`. A "verifier" can be compared manually against a MitM who guessed the code (`client-protocol.md`).
- **Transit:**
  - Abilities are `direct-tcp-v1`, `relay-v1` and `tor-tcp-v1`. Each side listens on a port and sends hints for every local address. The handshake strings use HKDF-derived tokens. The Sender writes `go\n` on the first connection that delivers the receiver's handshake, and closes the others.
  - With viable direct hints the client "will wait a few seconds before attempting to use the relay."
  - The relay pairs two connections by a token and "wires the connections together."
  - Records cost 44 bytes of overhead each (4-byte length, 24-byte nonce, 16-byte MAC). Nonces count up sequentially, and a lost, reordered or corrupt record drops the connection (`transit.md`).
- **File transfer:**
  - An `offer` carries a `file` (name, size) or a `directory` (`zipfile/deflated`, with zip size).
  - The receiver acks over transit with `{"ack":"ok","sha256":…}` (`file-transfer-protocol.md`).
  - The Python sender feeds Twisted `basic.FileSender` (`CHUNK_SIZE = 2**14`) into the record pipe (`cli/cmd_send.py`, Twisted `protocols/basic.py`).
  - **[I]** The per-record overhead is 44 / 16384 ≈ 0.27%.
- **Dilation:**
  - Layered L2–L5 with numbered generations of competing connections. The L3 frames are Noise `NNpsk0` with a PSK derived from the PAKE key.
  - L4 OPEN/DATA/CLOSE messages carry sequence numbers and ACKs. "If a new L3 connection is made, all queued messages will be re-sent (in seqnum order)", and subchannels pause and resume across reconnects.
  - **[I]** That gives connection-level durability. The file-transfer protocol on top of it still has no resume.
- **Resume:** [#88](https://github.com/magic-wormhole/magic-wormhole/issues/88) "restart interrupted transfers" has been open since 2016. [#722](https://github.com/magic-wormhole/magic-wormhole/issues/722) (2026, v0.24.0) [U]: a large directory "stalls at 100%", and after Ctrl-C "not a single transferred file can be found on the receiver", because directories arrive as one zip.

### Speeds

- warner (author), [#381](https://github.com/magic-wormhole/magic-wormhole/issues/381), 2020 [P]: "I can get at least tens of MBps (note bytes) with a direct connection." Via relay, "folks getting 20-50Mbps (note bits) on a regular basis, so maybe 2-7MBps". No NAT hole-punching yet (#6). The reporter saw 450 kB/s via relay.
- [#479](https://github.com/magic-wormhole/magic-wormhole/issues/479), 2023 [U]: on a 2010 Core 2 Duo receiver, the flamegraph showed "most of its time in NaCl", single-threaded.

### Sources

- https://github.com/magic-wormhole/magic-wormhole-protocols/blob/main/client-protocol.md , …/transit.md , …/file-transfer-protocol.md , …/dilation-protocol.md
- https://github.com/magic-wormhole/magic-wormhole/blob/master/src/wormhole/cli/cmd_send.py , https://github.com/magic-wormhole/magic-wormhole/blob/master/src/wormhole/cli/public_relay.py , https://github.com/twisted/twisted/blob/trunk/src/twisted/protocols/basic.py
- Issues: https://github.com/magic-wormhole/magic-wormhole/issues/88 , /381 , /479 , /722

---

## 10. Physical limits

### 10.1 Application-level TCP throughput on Wi-Fi, 2x2 client

- [P] Cisco's 802.11ac/ax throughput guidance: "Expected throughput = Data Rate x 0.7", and "65 to 70% of protocol efficiency" for a single client in a clean, large-transfer test. Its example: a 2-stream Wi-Fi 6 phone on 20 MHz with a 240–280 Mbit/s data rate yields ~160–200 Mbit/s.
- [P] Stute et al. measured ~546 Mbit/s TCP on a 2-stream 802.11ac AP at an 866.7 Mbit/s PHY rate, a ratio of 0.63. That matches the rule.

Standard 2-stream PHY rates and the [I] rule-of-thumb result (×0.65–0.70):

| Standard, 2x2 | Max PHY rate | Expected single-client TCP | In MB/s |
|---|---|---|---|
| 802.11n, 40 MHz, SGI, MCS15 | 300 Mbit/s | 195–210 Mbit/s | 24–26 |
| 802.11ac, 80 MHz, MCS9, SGI | 866.7 Mbit/s | 563–607 Mbit/s (measured 546) | 70–76 (68 measured) |
| 802.11ax, 80 MHz, MCS11, 0.8 µs GI | 1201 Mbit/s | 781–841 Mbit/s | 98–105 |
| 802.11ax, 160 MHz, MCS11 | 2402 Mbit/s | 1561–1681 Mbit/s | 195–210 |

These are upper bounds at close range with maximum MCS. The user-reported LocalSend and Quick Share figures above (30–110 MB/s at best on Wi-Fi 6) fit inside them.

### 10.2 Why phone → AP → PC (both on Wi-Fi) costs airtime twice

- [I] When both stations are associated to the same AP radio and channel, each payload frame is sent on air twice: STA→AP, then AP→STA. They share one collision domain, so usable end-to-end throughput is at most about half the single-hop figure. For 2x2 802.11ac that is ≈ 35–38 MB/s instead of 70–76.
- Cases that avoid this:
  - A PC wired to the AP: one wireless hop.
  - A tri-band AP putting the two stations on different radios: no shared airtime; the slower link bounds the transfer.
  - A direct link (Wi-Fi Direct group, phone hotspot, AWDL, TDLS): one hop.
- Supporting evidence:
  - TDLS (802.11z) exists to avoid exactly this relay hop. The Wi-Fi Alliance certification announcement says TDLS removes "the need to transmit data through the access point" [P, InfoWorld 2012].
  - Stute et al.'s measurement shows two concurrent flows sharing one channel splitting ~529 Mbit/s between them (§6).
  - Quick Share's 30–40 MB/s, peaks 90–110 MB/s, were on an upgraded direct link [U].

### 10.3 USB tethering

- [P] USB 2.0 signals at 480 Mbit/s with a theoretical maximum of ~53 MB/s of data (Microchip, cited by Wikipedia). For USB 3.0 at 5 Gbit/s, "it is realistic for about two-thirds of the raw throughput, or 330 MB/s" to reach an application (USB 3.0 spec, cited by Wikipedia).
- [P] OpenWrt wiki: "RNDIS is the most common protocol for Android-based devices for tethering via USB"; "newer devices may use NCM, others may require EEM".
- I found **no controlled local-transfer benchmark of RNDIS vs NCM**. The one user report (OpenWrt forum, 2023, Infinix Hot 20 5G) shows 275–300+ Mbit/s *Internet* throughput over USB 2.0 RNDIS. That figure is bound by cellular, not by USB.
- Apple's spec, as reported by 9to5Mac: iPhone 15 / 15 Plus are USB 2; 15 Pro is "USB 3 (up to 10Gb/s)" with a USB 3 cable, and the bundled cable is USB 2.
- [I] A USB 2.0 tether caps a transfer at < 53 MB/s, often less after RNDIS framing. That is lower than good 5 GHz single-hop Wi-Fi, so USB is only a speed win at USB 3 rates.

### 10.4 iOS constraints [P]

- **Multicast entitlement:** `com.apple.developer.networking.multicast`: "Your app must have this entitlement to send or receive IP multicast or broadcast on iOS. It also allows your app to browse and advertise arbitrary Bonjour service types." It "requires permission from Apple."
- **TN3179 table:**
  - Sending or receiving UDP multicast or broadcast: entitlement required.
  - UDP unicast: not required.
  - Browsing all service types or arbitrary Bonjour types: required.
  - Normal Bonjour register/browse/resolve needs Local Network access plus the types listed in `NSBonjourServices`, but no multicast entitlement.
  - "The multicast entitlement isn't required on macOS."
- **Local Network privacy:** the first access triggers a prompt with the `NSLocalNetworkUsageDescription` text. "If an iOS app is in the background and performs a local network operation while its Local Network privilege is undetermined, the system denies that operation without presenting the local network alert." App Clips cannot perform local network operations, and the simulator does not model this.
- **Background:** after `applicationDidEnterBackground` the app has "five seconds". `beginBackgroundTask` gives "extra time" (the limit is readable via `backgroundTimeRemaining`).
- **iOS 26 `BGContinuedProcessingTask`:** a task that "starts in the foreground and can continue running in the background". It must start "in response to someone's action". It may "use the network", the system shows progress in a Live Activity, and the user can cancel. The system may terminate tasks that report little progress.
- **Joining networks:** `NEHotspotConfigurationManager` (Hotspot Configuration entitlement) can join a Wi-Fi network by SSID/passphrase, but "the device prompts the user for approval."
- **iOS 26 Wi-Fi Aware framework:** pairs and connects "with Wi-Fi Aware certified accessories", offering "high-bandwidth and low-latency data transfers" with simultaneous infrastructure use. Supported on iPhone 12 and later.
- [I] Whether Android phones' Wi-Fi Aware (NAN) interoperates with this framework is not verified.

### 10.5 Android constraints [P]

- **MulticastLock:** "Normally the Wifi stack filters out packets not explicitly addressed to this device. Acquring a MulticastLock will cause the stack to receive packets addressed to multicast addresses. Processing these extra packets can cause a noticeable battery drain."
- **LocalOnlyHotspot:** `startLocalOnlyHotspot` (API 26) creates a hotspot "that an application can use to communicate between co-located devices", with no Internet access. The SSID and credentials are delivered in `SoftApConfiguration`. It may fail with `ERROR_INCOMPATIBLE_MODE`, e.g. while tethering. `startLocalOnlyHotspotWithConfiguration` (custom config) was added only in API 36 and needs `NEARBY_WIFI_DEVICES`.
- **Wi-Fi Direct with custom credentials:** `WifiP2pConfig.Builder` (API 29) has `setNetworkName()`, which must begin with "DIRECT-xy", and `setPassphrase()`, which takes 8–63 ASCII characters for legacy connection types.
- [I] Because of that builder, an Android phone can host a group that a Windows PC or an iPhone joins as an ordinary WPA2 network, e.g. after scanning a QR code with the SSID and passphrase.
- **Foreground services:**
  - Type `dataSync` (permission `FOREGROUND_SERVICE_DATA_SYNC`) covers "Data upload or download"/"Local file processing".
  - dataSync and mediaProcessing FGS may run "for a total of 6 hours in a 24-hour period", after which `Service.onTimeout()` is called (Android 15).
  - User-initiated data transfer jobs (`setUserInitiated(true)`, `RUN_USER_INITIATED_JOBS`, Android 14 / API 34) "require a notification, start immediately, and may be able to run for an extended period".
  - LocalSend v1.18 starts its foreground notification "while the app is still in the foreground, because Android 12+ rejects starting a foreground service from the background" (send_provider.dart).

### Sources

- https://www.cisco.com/c/en/us/support/docs/wireless-mobility/wireless-lan-wlan/212892-802-11ac-wireless-throughput-testing-and.html ; https://arxiv.org/abs/1808.03156 ; PHY rate tables: https://en.wikipedia.org/wiki/IEEE_802.11ac-2013 , https://en.wikipedia.org/wiki/Wi-Fi_6
- https://www.infoworld.com/article/2615053/wide-area-networking/wi-fi-alliance-starts-certifying-tunnel-technology-for-better-wireless-performa.html
- https://en.wikipedia.org/wiki/USB_hardware ; https://openwrt.org/docs/guide-user/network/wan/smartphone.usb.tethering ; https://forum.openwrt.org/t/slow-speed-on-usb-tethering-with-5g-phone/157510 ; https://9to5mac.com/2023/09/12/iphone-15-pro-supports-10gbps-usb-3-speeds/
- https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.developer.networking.multicast ; https://developer.apple.com/documentation/technotes/tn3179-understanding-local-network-privacy ; https://developer.apple.com/documentation/uikit/extending-your-app-s-background-execution-time ; https://developer.apple.com/documentation/backgroundtasks/bgcontinuedprocessingtask ; https://developer.apple.com/documentation/backgroundtasks/performing-long-running-tasks-on-ios-and-ipados ; https://developer.apple.com/documentation/networkextension/nehotspotconfigurationmanager ; https://developer.apple.com/documentation/wifiaware
- https://developer.android.com/reference/android/net/wifi/WifiManager.MulticastLock ; https://developer.android.com/reference/android/net/wifi/WifiManager#startLocalOnlyHotspot(android.net.wifi.WifiManager.LocalOnlyHotspotCallback,%20android.os.Handler) ; https://developer.android.com/reference/android/net/wifi/p2p/WifiP2pConfig.Builder ; https://developer.android.com/develop/background-work/services/fgs/service-types ; https://developer.android.com/develop/background-work/services/fgs/timeout ; https://developer.android.com/develop/background-work/background-tasks/uidt

---

## 11. Cipher cost

The question is whether end-to-end AEAD and per-chunk hashing can bottleneck at Wi-Fi rates (≤ ~210 MB/s best case for 2x2 Wi-Fi 6 at 160 MHz, §10.1).

### AEAD

| Measurement | Platform | Result | Type |
|---|---|---|---|
| rustls 0.23.37 (aws-lc-rs), TLS bulk transfer, "per second per core", 2026-03-07 | Xeon E-2386G (x86-64, AVX-512) | TLS1.2 AES-128-GCM: send 8134 MB/s, receive 7947 MB/s. TLS1.3 AES-256-GCM: 7421 / 7333 MB/s (OpenSSL 3.6.1: 5844–7129; BoringSSL 6218–8292) | [P] |
| rustls 0.23.15 (aws-lc-rs), 2024-10-31 | Ampere Altra Q80-30 (ARM64, Neoverse N1 class) | TLS1.2 AES-128-GCM: 2077 / 2173 MB/s. TLS1.3 AES-256-GCM: 1810 / 1936 MB/s | [P] |
| OpenSSL `aesni-gcm-x86_64.pl` (A. Polyakov) | Haswell / Broadwell / Skylake | AES-128-GCM 1.0 / 0.74 / 0.63 cycles/byte | [P] |
| OpenSSL `aesv8-armx.pl` + `ghashv8-armx.pl` | ARMv8 Crypto Extensions | AES-128-CTR 1.36 cpb (Cortex-A53), 0.89 (A57), 1.20 (Apple A7). GHASH (64-bit PMULL) 0.85 (A53), 0.73 (A57), 0.58 (A7) | [P] |
| OpenSSL `chacha-armv8.pl`, `poly1305-armv8.pl` | ARMv8 NEON | ChaCha20 4.06 cpb (A53), 4.15 (A57), 1.60 (A7, 6×NEON). Poly1305 1.47 (A53), 1.14 (A57), 0.72 (A7) | [P] |
| OpenSSL `chacha-x86_64.pl`, `poly1305-x86_64.pl` | Skylake | ChaCha20 1.19 cpb (AVX2), 0.80 (AVX-512VL). Poly1305 0.51 cpb (AVX2) | [P] |
| Cloudflare, 2016-04-04 | Haswell | AES-128/256-GCM beat ChaCha20-Poly1305 for records > 320 bytes; ChaCha20-Poly1305 "below 2 cycles/byte" | [P] |

[I] Derived single-core throughput (bytes/cycle × clock; CTR + GHASH summed, an upper bound on cost because stitched GCM code is faster):

- AES-128-GCM on Cortex-A53 at 2.0 GHz: 2.21 cpb → ~0.9 GB/s. Cortex-A57 at 2.0 GHz: 1.62 cpb → ~1.2 GB/s. Skylake at 3.0 GHz: 0.63 cpb → ~4.8 GB/s.
- ChaCha20-Poly1305 on Cortex-A53 at 2.0 GHz: 5.53 cpb → ~360 MB/s. Skylake at 3.0 GHz (AVX2): 1.70 cpb → ~1.8 GB/s.
- At the best-case Wi-Fi rate (~210 MB/s):
  - AES-GCM on an old A53 little core uses roughly a quarter of the core.
  - ChaCha20-Poly1305 on the same core uses well over half.
  - Both are a few percent of a desktop core.
- Current phone big cores (Cortex-X / Apple) are faster than these older cores. I found no per-core figure for them in these primary sources.

**Conclusion [I]:** a native AEAD is not the reason LocalSend measured 7 MB/s HTTPS vs 50 MB/s HTTP. The maintainer attributed that to the Dart HTTPS stack, and the move to a Rust client raised Android sending from 15 to 40 MB/s. croc's maintainer also found disabling encryption "so marginal". Prefer AES-GCM on hardware with AES/PMULL (all ARMv8 crypto-extension SoCs and x86 with AES-NI), and keep ChaCha20-Poly1305 for cores without it.

### Hashing

| Measurement | Platform | Result | Type |
|---|---|---|---|
| BLAKE3 benchmark chart data (`cargo bench '16384$'`) | AWS c5.metal, Xeon Platinum 8275CL (AVX-512), single thread, 16 KiB inputs | BLAKE3 6866 MiB/s; BLAKE2b 1312; SHA-1 1027; BLAKE2s 876; MD5 740; SHA-512 720; SHA-256 484 (OpenSSL); SHA3-256 394 | [P] |
| BLAKE3 paper | Cascade Lake-SP | Peak single-thread 4× BLAKE2b, 8× SHA-512, 12× SHA-256; scales with threads (Rayon) | [P] |
| BLAKE3 paper | 32-bit ARM1176 (Raspberry Pi Zero) | 1.3× SHA-256, 3× BLAKE2b/SHA-512 | [P] |
| BLAKE3 PR #319 (contributor sdlyyxy, 2023) | Apple M1, `b3sum` reading stdin from a pipe | 1.35 → 1.50 GB/s after NEON rotation fix | [P, contributor measurement] |
| xxHash README | i7-9700K | XXH64 19.4 GB/s; XXH3 (AVX2) 59.4 GB/s; memcpy 28 GB/s | [P] |
| Buildbarn maintainer, 2024 | Apple M1 | Could not get BLAKE3 "anywhere near SHA-256" (hardware SHA-2) | [U] |

[I] Notes on hashing:

- Per-chunk BLAKE3 at Wi-Fi rates costs a small fraction of a core on x86. On ARM it is roughly 1.5 GB/s per core (M1 figure above), still about 7× the best-case Wi-Fi rate.
- SHA-256 without hardware instructions (484 MiB/s above) would use ~40% of a core at 200 MB/s. With ARMv8 SHA-2 or x86 SHA-NI it is usually not the bottleneck.
- The measurable cost in existing tools is the **extra read pass**, not the hash function: LocalSend 1.18's 3-minute pre-hash (#3559), and croc's pre-hash plus verify read (#635).
- The tree structure of BLAKE3/Bao allows hashing while sending and verified streaming of chunks, which removes the separate pass.

### Sources

- https://rustls.dev/perf/2026-03-07-report/ ; https://rustls.dev/perf/2024-10-31-arm64/
- https://github.com/openssl/openssl/blob/master/crypto/modes/asm/aesni-gcm-x86_64.pl , …/crypto/aes/asm/aesv8-armx.pl , …/crypto/modes/asm/ghashv8-armx.pl , …/crypto/chacha/asm/chacha-armv8.pl , …/crypto/poly1305/asm/poly1305-armv8.pl , …/crypto/chacha/asm/chacha-x86_64.pl , …/crypto/poly1305/asm/poly1305-x86_64.pl
- https://blog.cloudflare.com/it-takes-two-to-chacha-poly/
- https://github.com/BLAKE3-team/BLAKE3-specs/blob/master/blake3.pdf ; https://github.com/BLAKE3-team/BLAKE3-specs/blob/master/benchmarks/bar_chart.py ; https://github.com/BLAKE3-team/BLAKE3/pull/319 ; https://github.com/Cyan4973/xxHash ; https://github.com/buildbarn/bb-storage/issues/207

---

## 12. Design implications for SweaxDrop

Each item names a weakness observed above and the evidence for it.

- **One request or connection per file adds per-file latency.**
  - Evidence: LocalSend issues one HTTP POST per file with ≤2 in flight (§1); "cannot transfer more than two files per second" (#1657). KDE Connect opens a new TLS connection per file (§2). croc negotiates each file in sequence: 16 files / 2.4 MB > 10 s (#584).
  - Design: one long-lived encrypted session per transfer. Send metadata for all files up front, then one continuous framed stream (or a few streams) carrying many files back-to-back, with no round trip per file.
- **There is no resume, and some tools destroy partial data.**
  - Evidence: LocalSend, KDE Connect and PairDrop have none. Warpinator deletes partial files on restart. magic-wormhole lacks resume (#88) and loses whole zipped directories (#722). croc's working resume relies on zero-chunk detection.
  - Design: a chunk manifest with per-chunk hashes and a persisted receiver bitmap; resume by offset; write to a temp file and rename atomically per file.
- **A pre-hash pass delays the start of the transfer.**
  - Evidence: LocalSend 1.18 took 3 min of hashing for 1 min of transfer (#3559); croc pre-hashes and re-reads to verify (#635).
  - Design: hash while sending (BLAKE3 tree, verified per chunk), and send the file's root hash in the trailer.
- **Sampling hashes and transport-only integrity leave gaps.**
  - Evidence: croc's `imohash` samples rather than covers every byte. KDE Connect and Warpinator have no file-level hash.
  - Design: per-record AEAD covers the wire. The content hash covers disk read/write paths and gives resume a ground truth.
- **Peer identity goes unverified or is weakly verified.**
  - Evidence: LocalSend accepted any certificate up to v1.17, and since v1.18 it pins a fingerprint learned at discovery that no user ever compares (§1). Warpinator's default group code is the public string "Warpinator" (§3). PairDrop trusts its signaling server. AirDrop silently falls back to unauthenticated connections (§6). Models that work: KDE Connect's 8-hex verification key, the PAKE in croc and magic-wormhole, Quick Share's 4-digit PIN from UKEY2.
  - Design: first contact via PAKE (short code or QR, e.g. SPAKE2/CPace), then pinned long-term keys, plus a short authentication string the user can compare.
- **Small user-space buffers and stop-and-wait flow control cap throughput.**
  - Evidence: KDE Connect desktop writes 4 KiB and waits for the TLS buffer to drain (§2). PairDrop acks every 1 MB (§4). The Python magic-wormhole writes 16 KiB records (§9).
  - Design: large writes (≥256 KiB–1 MiB per syscall batch), a credit-based window sized to bandwidth × RTT, and no per-chunk acks.
- **Parallel TCP connections help, but only modestly and depending on conditions.**
  - Evidence: croc 78 s vs 92 s (−15%) and 105 s vs 115 s (#602).
  - Design: support 1–4 streams, choose adaptively, and benchmark per platform rather than hard-coding a number.
- **The slowness comes from the TLS/HTTP implementation, not from cryptography.**
  - Evidence: LocalSend HTTPS 7 MB/s vs HTTP 50 MB/s under Dart, and 15 → 40 MB/s with a Rust client (§1). Native AES-GCM runs ≥ ~0.9 GB/s per core even on Cortex-A53 (§11).
  - Design: keep encryption always on in native Rust (rustls/aws-lc-rs or ring, AES-GCM preferred, ChaCha20-Poly1305 fallback). There is no need for an "encryption off" mode.
- **Discovery is fragile.**
  - Evidence: iOS needs a special entitlement for multicast and broadcast (TN3179). Bonjour with declared `NSBonjourServices` works without it. Android filters multicast unless a `MulticastLock` is held. Routers with AP isolation, Windows "public" network profiles and VPNs break discovery (LocalSend troubleshooting).
  - Design: mDNS/Bonjour with a fixed service type as the primary path on all platforms; UDP multicast optional; always offer QR / manual-IP pairing.
- **A double wireless hop halves throughput.**
  - Evidence: phone→AP→PC on one channel (§10.2). Quick Share's direct-link upgrade reached 30–40 MB/s with peaks of 90–110 MB/s (§5).
  - Design: a "direct link" mode. Android hosts a Wi-Fi Direct group with a custom SSID and passphrase (API 29 builder), shared by QR, which the PC joins natively and the iPhone joins via `NEHotspotConfiguration` (one user prompt). Evaluate iOS 26 Wi-Fi Aware later.
- **Long transfers are killed in the background.**
  - Evidence: iOS gives 5 s plus limited `beginBackgroundTask` time; iOS 26 adds `BGContinuedProcessingTask` for user-initiated work with a Live Activity. Android limits dataSync FGS to 6 h per 24 h (Android 15), and UIDT jobs exist since API 34. A LocalSend user saw transfers stop with the screen off (#1657).
  - Design: per-platform background strategy, plus resume as the safety net.
- **Android storage paths are slow.**
  - Evidence: the LocalSend README lists slow SAF streaming (`saf_stream`) as a known issue; v1.18 switched to file descriptors. A user reported an Android temp-dir copy after receive (#384).
  - Design: stream directly from and to file descriptors, write to the final location, and rename.
- **Browser fallbacks are slow or unencrypted.**
  - Evidence: LocalSend's web share is plain HTTP because browsers reject self-signed certificates. WebRTC data channels are window- and RTT-limited (§4).
  - Design: if a browser path is offered, label its security clearly and expect lower speed. Use 256 KiB messages with `bufferedAmountLowThreshold` flow control.
- **Some tools depend on an Internet relay even on a LAN.**
  - Evidence: magic-wormhole always needs its mailbox server. croc uses a public relay unless `--local`.
  - Design: a LAN-only mode that works with no Internet at all, and an optional relay only for cross-network use.
