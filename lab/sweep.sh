#!/usr/bin/env bash
# sweep.sh: ends every process of mine whose private HOME is a lab run's home (by environ), nothing else.
n=0
for d in /proc/[0-9]*; do
  p="${d#/proc/}"; [ "$p" = "$$" ] && continue
  h="$(tr '\0' '\n' <"$d/environ" 2>/dev/null | grep -m1 '^HOME=')" || continue
  case "$h" in "HOME=$HOME/wyss-lease-lab/runs/"*) kill -TERM "$p" 2>/dev/null && { n=$((n+1)); echo "TERM $p $(tr '\0' ' ' <$d/cmdline | cut -c1-70)"; };; esac
done
echo "swept=$n"
