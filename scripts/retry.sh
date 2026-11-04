#!/bin/sh
# Runs a command up to 5 times, waiting 10, 20, 30, 40 s in between: GitHub's API sometimes
# answers 502 for a while, which shouldn't fail a whole release.
n=1
until "$@"; do
  [ "$n" -ge 5 ] && exit 1
  echo "retry.sh: attempt $n failed, trying again in $((n * 10)) s" >&2
  sleep $((n * 10))
  n=$((n + 1))
done
