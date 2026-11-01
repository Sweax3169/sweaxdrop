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

**Runs.** Three runs per case, median reported. Received files are deleted between
runs.

## Results

<!-- RESULTS -->

## TCP or QUIC

Network path only (the sender generates the bytes, the receiver verifies and
discards them), 2 GiB, `sweaxdrop-bench loopback`, median of three:

<!-- TRANSPORT -->

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
