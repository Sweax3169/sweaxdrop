#!/usr/bin/env bash
# The published comparison: SweaxDrop and LocalSend 1.18.2 (official CLI), same
# files, same drive, same machine. Runs are interleaved (SweaxDrop, LocalSend,
# SweaxDrop, ...) so background load on the PC hits both programs alike.
#   tools/run_benchmarks.sh <dir with localsend-1.18.2/ and data/> [rounds]
set -u
T="${1:?tools dir}"
R="${2:-3}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# Both programs run from the same drive as the data: an executable started
# from another drive (here a hard disk) made Windows create files 4-7x slower.
mkdir -p "$T/bin"
cp "$ROOT/target/release/sweaxdrop-bench.exe" "$T/bin/sweaxdrop-bench.exe"
BENCH="$T/bin/sweaxdrop-bench.exe"
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

# Warm-up (not recorded): the first run of a fresh executable is slowed down
# while Windows Defender looks at it.
"$BENCH" loopback "${P300[@]}" --disk --streams 4 --repeat 1 --case warmup >/dev/null
python "$ROOT/tools/localsend_bench.py" "$CLI" "$T/ls-work" warmup "${SMALL300[@]}" >/dev/null

settle() { sleep 8; }   # let the drive finish writing before the next run

# Rounds alternate who goes first (A B, then B A), so neither program always
# runs right after the other one's big writes.
pair() { # name, sweaxdrop command, localsend command
  if [ $((i % 2)) -eq 1 ]; then eval "$2"; settle; eval "$3"; else eval "$3"; settle; eval "$2"; fi; settle
}
for i in $(seq 1 "$R"); do
  echo "== round $i"
  pair huge 'sx --path "$HUGE" --case "4 GiB file"' 'ls_cli "4 GiB file" "$HUGE"'
  pair photos 'sx --path "$T/data/photos" --case "200 photos (723 MB)"' 'ls_cli "200 photos (723 MB)" "${PHOTOS[@]}"'
  pair s300 'sx "${P300[@]}" --case "300 small files"' 'ls_cli "300 small files" "${SMALL300[@]}"; settle; ls_proto "300 small files" "${SMALL300[@]}"'
  pair s10k 'sx --path "$T/data/small" --case "10 000 small files (338 MB)"' 'ls_proto "10 000 small files (338 MB)" --folder "$T/data/small"'
done
ls_proto "4 GiB file" "$HUGE"
echo done
