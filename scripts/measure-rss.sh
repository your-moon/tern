#!/usr/bin/env bash
# Measure resident memory of a running tern process over a short window.
# Usage: scripts/measure-rss.sh <pid> [seconds]   → prints peak and final RSS in MB.
set -euo pipefail
pid=$1; secs=${2:-10}; peak=0; rss=0
for ((i = 0; i < secs; i++)); do
  rss=$(ps -o rss= -p "$pid" | tr -d ' ') || { echo "process $pid gone"; exit 1; }
  (( rss > peak )) && peak=$rss
  sleep 1
done
printf 'final_rss_mb=%.1f peak_rss_mb=%.1f\n' "$(bc -l <<<"$rss/1024")" "$(bc -l <<<"$peak/1024")"
