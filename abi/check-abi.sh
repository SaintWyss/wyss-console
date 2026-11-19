#!/usr/bin/env bash
# Fails if the installed libghostty-vt headers disagree with the constants/sizes in src/bin/wyss-console-native/ghostty.rs.
# Run whenever the installed Ghostty package changes.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"; scratch="${WYSS_TEST_SCRATCH:-$HOME/.cache/wyss-build}"; mkdir -p -- "$scratch"; out="$(mktemp -d "$scratch/abi.XXXXXX")"; trap 'rm -r "$out"' EXIT
cc -o "$out/abi" "$here/abi.c"
"$out/abi" | tr ' ' '\n' | grep = | sort > "$out/c.txt"
{ grep -oE '^const [A-Z_0-9]+: [a-z0-9_]+ = [0-9]+' "$here/../src/bin/wyss-console-native/ghostty.rs" | awk '{print $2 $5}' | sed 's/:/=/'
  echo "style=72"; echo "cursor=24"; echo "scroll=24"; echo "mode=4"; echo "rgb=3"; echo "gridref=24"; echo "selection=64"; echo "selbuf=24"; echo "coord=8"; echo "string=16"; } | grep -vE '^(SUCCESS|MAX_TEXT)=' | sort > "$out/rs.txt"
if diff <(cut -d= -f1 "$out/rs.txt" | xargs -I{} grep "^{}=" "$out/c.txt") "$out/rs.txt"; then echo "abi OK ($(wc -l < "$out/rs.txt") valores)"; else echo "ABI DISTINTA: actualizar src/bin/wyss-console-native/ghostty.rs" >&2; exit 1; fi
