#!/usr/bin/env bash
# Keep one differential-fuzz campaign alive for a wall-clock budget (M2.13).
# Usage: fuzz_campaign.sh HOURS START_SEED [JOBS]
#
# Each run gets the remaining time budget; if the Python driver exits early the
# supervisor resumes at the seed after the last printed one, so a crash costs
# minutes, not the whole gate. Output is appended to fuzz.log in the current
# directory; the campaign ends when the deadline passes.
set -u
set -o pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

hours=${1:?usage: fuzz_campaign.sh HOURS START_SEED [JOBS]}
seed=${2:?usage: fuzz_campaign.sh HOURS START_SEED [JOBS]}
jobs=${3:-4}
python=${PYTHON:-python3}
deadline=$(awk -v start="$(date +%s)" -v hours="$hours" 'BEGIN { printf "%d", start + hours * 3600 }')
log=fuzz.log

while :; do
  now=$(date +%s)
  [ "$now" -ge "$deadline" ] && break
  remaining=$(( deadline - now ))
  budget=$(awk "BEGIN { printf \"%.4f\", $remaining / 3600 }")
  "$python" scripts/diff_fuzz.py --duration "$budget" --jobs "$jobs" --start "$seed" | tee -a "$log"
  status=$?
  last=$(sed -n 's/^seed \([0-9]*\):.*/\1/p' "$log" | tail -1)
  [ -n "$last" ] && seed=$(( last + 1 ))
  # A clean exit means the budget ran out; anything else is worth a restart line.
  [ "$status" -eq 0 ] && continue
  echo "supervisor: driver exited $status at seed $seed, restarting" | tee -a "$log"
  sleep 5
done
