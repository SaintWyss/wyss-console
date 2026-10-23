#!/bin/sh
# Three comparable runs of each shape of M1-13 on the isolated display, one after another.
#   benchmarks/m1_gui_runs.sh BINARY TEST_BINARY OUT_DIR
# Each run makes and removes its own display, host, state and home; nothing of the owner's is
# named. A failed run is kept as it ended (its JSON says how far it got) and the next one starts.
set -u
binary=$1
tests=$2
out=$3
for shape in a b c; do
  for run in 1 2 3; do
    python3 -I "$(dirname "$0")/m1_gui.py" --binary "$binary" --test-binary "$tests" \
      --shape "$shape" --run "$run" --out "$out/shape-$shape" \
      --idle-seconds 60 --switches 40 --keys 40 || echo "run $shape/$run ended with $?" >&2
  done
done
