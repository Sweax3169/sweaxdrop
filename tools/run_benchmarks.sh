#!/usr/bin/env bash
# The published comparison: SweaxDrop and LocalSend 1.18.2 (official CLI), same
# files, same drive, same machine. Runs are interleaved (SweaxDrop, LocalSend,
# SweaxDrop, ...) so background load on the PC hits both programs alike.
#   tools/run_benchmarks.sh <dir with localsend-1.18.2/ and data/> [rounds]
set -u
T="${1:?tools dir}"
R="${2:-3}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BENCH="$ROOT/target/release/sweaxdrop-bench.exe"
CLI="$T/localsend-1.18.2/LocalSend-CLI-1.18.2-windows-x86-64.exe"
OUT="$ROOT/bench/results"
mkdir -p "$OUT" "$T/sx-work" "$T/ls-work"
export SWX_BENCH_INBOX="$T/sx-work"
export PYTHONIOENCODING=utf-8
HUGE="$(ls "$T"/data/huge/*.bin | head -1)"
mapfile -t PHOTOS < <(ls "$T"/data/photos/*.jpg)
mapfile -t SMALL300 < <(ls "$T"/data/small/d00/*.dat "$T"/data/small/d01/*.dat "$T"/data/small/d02/*.dat | head -300)
P300=(); for f in "${SMALL300[@]}"; do P300+=(--path "$f"); done

sx() { "$BENCH" loopback "$@" --disk --streams 4 --repeat 1 --out "$OUT/compare.json"; }
ls_cli() { python "$ROOT/tools/localsend_bench.py" "$CLI" "$T/ls-work" "$@" | tee -a "$OUT/localsend.jsonl"; }
ls_proto() { python "$ROOT/tools/localsend_bench.py" "$CLI" "$T/ls-work" "$1" --sender proto --bench "$BENCH" "${@:2}" | tee -a "$OUT/localsend.jsonl"; }

for i in $(seq 1 "$R"); do
  echo "== round $i"
  sx --path "$HUGE" --case "4 GiB file"
  ls_cli "4 GiB file" "$HUGE"
  sx --path "$T/data/photos" --case "200 photos (723 MB)"
  ls_cli "200 photos (723 MB)" "${PHOTOS[@]}"
  sx "${P300[@]}" --case "300 small files"
  ls_cli "300 small files" "${SMALL300[@]}"
  ls_proto "300 small files" "${SMALL300[@]}"
  sx --path "$T/data/small" --case "10 000 small files (338 MB)"
  ls_proto "10 000 small files (338 MB)" --folder "$T/data/small"
done
ls_proto "4 GiB file" "$HUGE"
echo done
