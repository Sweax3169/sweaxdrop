#!/usr/bin/env bash
# Network path only: 2 GiB generated on the fly, verified and discarded on the
# other side. Lanes 1-8 over TCP and QUIC, and ChaCha20 instead of AES.
#   tools/run_network.sh <bench.exe> [repeat]
set -u
B="${1:?sweaxdrop-bench.exe}"
R="${2:-3}"
OUT="$(cd "$(dirname "$0")/.." && pwd)/bench/results/network.json"
for t in tcp quic; do
  for s in 1 2 4 8; do
    "$B" loopback --bytes 2G --streams $s --transport $t --repeat "$R" --case "2 GiB in memory" --out "$OUT"
  done
done
"$B" loopback --bytes 2G --streams 4 --cipher chacha --repeat "$R" --case "2 GiB in memory" --out "$OUT"
