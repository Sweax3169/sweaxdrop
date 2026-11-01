# Benchmarks

Every number in the README, on the website and below was measured with the tools
in this repository. Nothing is estimated. Raw results: [`bench/results`](../bench/results).

## Machine

| | |
|---|---|
| CPU | AMD Ryzen 5 7600 (6 cores, 12 threads) |
| Drive | Lexar NM620 1 TB NVMe SSD (source files and received files) |
| OS | Windows 11 Pro 26200, Microsoft Defender real-time protection on |
| SweaxDrop | 1.0.0, release build |
| LocalSend | 1.18.2, official command-line app (`LocalSend-CLI-1.18.2-windows-x86-64.exe`, SHA-256 `ca0b267e…dc38`) |

## Method

**Same path for both.** Both programs send from and to the same PC. Received files
are written to the same NVMe drive the source files are on. This measures what each
program can move when the radio is not the limit: protocol, encryption, hashing,
disk handling and per-file overhead.

**SweaxDrop** runs as two nodes in one process (`sweaxdrop-bench loopback --disk`),
over TCP on 127.0.0.1, 4 lanes, 4 MiB chunks, TLS 1.3 with AES-128-GCM, BLAKE3 on
every chunk, every file written and renamed into place. The sender trusts the
receiver (as after pairing), so there is no prompt. The clock starts when the send
is requested and stops when the sender has the receiver's receipt, which is sent
after every file is closed under its final name.

**LocalSend** runs as two official CLI processes (`tools/localsend_bench.py`), each in
a pseudo terminal: the sender is started with the files, Enter is pressed on its
device list, and the receiver gets `y` the moment the request appears (it is
answered in 40–90 ms; LocalSend's GUI would do the same with Quick Save). LocalSend
1.18 uses HTTPS and hashes every file with SHA-256 before sending, by default. The
clock starts when Enter is pressed and stops when every byte of every file is in
the receiver's folder, polled every 20 ms.

**Folders the CLI cannot take.** The LocalSend CLI takes files as `-f` arguments, and
10 000 paths do not fit on a Windows command line. For that case the official CLI
still receives, and the sender is `sweaxdrop-bench localsend`: a Rust implementation
of LocalSend's v2 protocol (`prepare-upload`, then one HTTPS `upload` per file) with
LocalSend 1.18's two uploads at a time. It does not hash files first, so it is at
least as fast as LocalSend's own sender. Where both senders can be used (the 4 GiB
file, 300 files) both were run as a cross-check.

**Same drive for the programs, too.** Both executables run from the same NVMe
drive as the data. This matters on Windows more than one would think: the same
`sweaxdrop-bench.exe` created small files 4–7 times slower when it was started
from a hard disk (`D:`) than from the SSD, even though the files themselves were
written to the SSD in both cases (`sweaxdrop-bench fstest` measures this). An
early run that started SweaxDrop from the hard disk and LocalSend from the SSD
made SweaxDrop look slower on 10 000 files; it was discarded.

**Warm-up.** The first run of a freshly copied executable was much slower than the
ones after it (23.6 s against 3.9 s for the 10 000 files; most likely Windows
Defender examining the new binary), so each program does one unrecorded run
first. Inside each SweaxDrop measurement process, one unrecorded transfer of 64
small files goes first as well: an app that stays open pays its one-time costs
(thread pools, the first file writes of a new process) once, and without this
the first measured run of each process paid them (0.67 s against 0.10–0.13 s
for the same 300 files).

**Order and rounds.** The PC had other applications open, so the cases run in
four rounds. Rounds alternate which program goes first (SweaxDrop then LocalSend,
then LocalSend then SweaxDrop), and every run is followed by 8 seconds of rest
so the drive finishes writing before the next one starts. The median of the four
runs is reported. Received files are deleted between runs.

## Results

<!-- RESULTS:START -->
| Files | SweaxDrop 1.0.0 | LocalSend 1.18.2 (CLI) | LocalSend 1.18.2 (protocol sender) | SweaxDrop is |
|---|---|---|---|---|
| One 4 GiB file | **13.57 s** | 13.56 s | 10.50 s (1 run) | about the same |
| 200 photos, 723 MB | **0.64 s** | 3.08 s | — | **4.8× faster** |
| 10 000 small files, 338 MB | **5.24 s** | — | 15.62 s | **3.0× faster** |
| 300 small files | **0.13 s** | 1.67 s | 0.43 s | **3.4× faster** |

Every run, in order, is in `bench/results/compare.json` (SweaxDrop) and `bench/results/localsend.jsonl` (LocalSend).
<!-- RESULTS:END -->

## TCP or QUIC

Network path only (the sender generates the bytes, the receiver verifies and
discards them), 2 GiB, `sweaxdrop-bench loopback`, median of three:

<!-- TRANSPORT:START -->
| Transport | Cipher | Lanes | Throughput | CPU (both ends) |
|---|---|---|---|---|
| TCP | AES-128-GCM | 1 | 1,080 MB/s | 1.9 s per GB |
| TCP | AES-128-GCM | 2 | 1,374 MB/s | 1.8 s per GB |
| TCP | AES-128-GCM | 4 | 3,063 MB/s | 1.8 s per GB |
| TCP | AES-128-GCM | 8 | 3,533 MB/s | 1.9 s per GB |
| QUIC | AES-128-GCM | 1 | 225 MB/s | 9.8 s per GB |
| QUIC | AES-128-GCM | 2 | 188 MB/s | 10.7 s per GB |
| QUIC | AES-128-GCM | 4 | 213 MB/s | 10.8 s per GB |
| QUIC | AES-128-GCM | 8 | 199 MB/s | 10.7 s per GB |
| TCP | ChaCha20-Poly1305 | 4 | 2,300 MB/s | 2.3 s per GB |
<!-- TRANSPORT:END -->

QUIC (quinn) on Windows is CPU-bound in user space; TCP gets checksum offload and
large sends from the kernel. TCP lanes are the default; QUIC stays available in
Settings for networks where it does better.

## Phones over Wi-Fi

Real-phone measurements need real phones on a real network. They are added here
with the phone model, the Wi-Fi standard and channel width, and whether the PC is
on Ethernet or Wi-Fi. The Android emulator was used only to check correctness:
a 200 MB file from the Android app to the PC and a folder of photos from the PC to
the app arrived byte-identical (SHA-256) and landed in the gallery; the emulator's
virtual network ran at about 3.5 MB/s, which says nothing about phones.

## Reproduce

```bash
cargo build --release -p sweaxdrop-bench
target/release/sweaxdrop-bench dataset --out <dir>/data --huge-gb 4
# put LocalSend-CLI-1.18.2-windows-x86-64.exe in <dir>/localsend-1.18.2/
pip install pywinpty
tools/run_benchmarks.sh <dir> 3
python tools/bench_report.py
```

The dataset is generated, not downloaded: one 4 GiB file, 200 files of 3–6 MB
("photos") and 10 000 files of 1–64 KB in 100 folders, all incompressible.
