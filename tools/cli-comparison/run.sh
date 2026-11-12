#!/bin/bash
# Usage: run.sh <cli> <out-dir> [fast]
# Runs the comparison cases (outputs, summaries, errors and exit codes of `slice`) into
# <out-dir>/cases and the refusals into <out-dir>/errors. "fast" skips the slow FX-12 case.
set -u
BIN=$(realpath "$1"); OUT=$2; FAST=${3:-}
S=$(cd "$(dirname "$0")" && pwd); I=$S/inputs; F=$(cd "$S/../../fixtures/reference/stl" && pwd)
rm -rf "$OUT"; mkdir -p "$OUT/cases" "$OUT/errors"; OUT=$(cd "$OUT" && pwd)
P="--printer $I/printer.json --resin $I/resin.json --no-progress"
PB="--printer $I/printer-big.json --resin $I/resin.json --no-progress"
C="--center --drop"
run() { local name=$1; shift; local t0; t0=$(date +%s.%N)
  "$BIN" slice "$@" >"$name.stdout" 2>"$name.stderr"; echo $? >"$name.exit"
  printf '%-4s exit %s  %.2fs\n' "$name" "$(cat "$name.exit")" "$(echo "$(date +%s.%N) - $t0" | bc)"; }
cd "$OUT/cases"
X=65.28; Y=41.31
run c01 --assume-mm $F/fx-01-cube.stl $P $C --format reference --output c01.out
run c02 --model $F/fx-11-high-complexity.stl --units mm $P $C --hollow 2 \
  --drain-hole "$(echo "$X+32" | bc)" $Y 30 -x 6 3 --drain-hole $X "$(echo "$Y-32" | bc)" 20 +y 9 3 \
  --format goo-v3 --output c02.goo
run c03 --model $F/fx-11-high-complexity.stl --units mm $P $C --antialias 4 --format ctb-v4-plain --output c03.ctb
run c04 --model $F/fx-10a-open.stl --units mm $P $C --format goo-v3 --output c04.goo
run c05 --model $F/fx-10a-open.stl --units mm $P $C --repair duplicates,orientation,fill-holes --format goo-v3 --output c05.goo
run c06 --model $F/fx-10a-open.stl --units mm $P $C --repair duplicates,orientation,fill-holes --hollow 2 --format goo-v3 --output c06.goo
run c07 --model $F/fx-09-reversed-face.stl --units mm $P $C --format goo-v3 --output c07.goo
[ -z "$FAST" ] && run c08 --model $F/fx-12-real-model.stl --units mm $P $C --format goo-v3 --output c08.goo
run c09 --model $F/fx-11-high-complexity.stl --units mm $PB --center --drop --format anycubic-pp1-v3 --output c09.pp1
run c10 --model $F/fx-11-high-complexity.stl --units mm $P $C --hollow 2 --hollow-simplify auto \
  --surface-drain-hole "$(echo "$X+30" | bc)" $Y 30 3 --format goo-v3 --output c10.goo
run c11 --model $F/fx-11-high-complexity.stl --units mm $P $C --rotate 30 15 0 --scale 0.5 0.6 0.7 --mirror x --format ctb-v5-plain --output c11.ctb
run c12 --model $F/fx-01-cube.stl --units mm $P --drop --copies 6 --spacing 4 --format goo-v3 --output c12.goo
run c13 --plate $I/plate.json $P --format anycubic-pwma-v516 --output c13.pwma
run c14 --model $F/fx-11-high-complexity.stl --units mm $P $C --scale 0.3 0.3 0.3 --layer-plan $I/layers.json --format goo-v3 --output c14.goo
run c15 --model $F/fx-01-cube.stl --units mm $P --center --drop --lift 4 --supports $I/supports.json --format goo-v3 --output c15.goo
run c16 --model $F/fx-07-internal-cavity.stl --units mm $P $C --antialias 2 --min-gray 64 --xy-compensation -1 --bottom-xy-compensation 2 --format goo-v3 --output c16.goo
run c17 --model $F/fx-07-internal-cavity.stl --units mm $P $C --fill strict --format photon-v1-firmware-motion --output c17.photon
run c18 --model $F/fx-11-high-complexity.stl --units mm $P $C --hollow 2 --drain-hole $X $Y 70 -z 5 3 --format goo-v3 --output c18.goo
run c19 --model $F/fx-01-cube.stl --units mm $P --format goo-v3 --output c19.goo
run c20 --model $F/fx-01-cube.stl --units mm $P $C --hollow 3 --hollow-cell 0.4 --format anycubic-pm5s-v518 --output c20.pm5s
cd "$OUT/errors"
run e1 --model $I/inside-out.stl --units mm $P --center --drop --format goo-v3 --output e1.goo
run e2 --model $I/inside-out.stl --units mm $P --translate 500 0 0 --format goo-v3 --output e2.goo
run e3 --model $I/inside-out.stl $P --center --drop --format goo-v3 --output e3.goo
run e4 --model $F/fx-10a-open.stl --units mm $P --center --drop --hollow 2 --format goo-v3 --output e4.goo
run e5 --model $F/fx-10a-open.stl --units mm $P --center --drop --fill strict --format goo-v3 --output e5.goo
run e6 --model $F/fx-10a-open.stl --units mm $P --center --drop --close-holes 0 --format goo-v3 --output e6.goo
run e7 --model $F/fx-09-reversed-face.stl --units mm $P --center --drop --repair duplicates --format goo-v3 --output e7.goo
run e8 --model $F/fx-01-cube.stl --units mm $P --center --translate 0 0 3 --format goo-v3 --output e8.goo
run e9 --model $I/inside-out.stl --units mm $P --center --drop --repair duplicates,orientation,outward --format goo-v3 --output e9.goo
