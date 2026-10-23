#!/usr/bin/env bash
# The app under perf, for `run.mjs --profile <dir>` (Linux): run.mjs gives
# this script to the driver as the app, and it starts the real one
# (PROFILE_APP) with perf following it and every process it starts (the web
# view's, the worker, ffmpeg).
#
# WebKitWebDriver ends each session by killing the process it started, at
# once (SIGKILL), so perf mustn't be that process: its file would be left
# unfinished. perf runs in a session of its own, attached to this process;
# once it says it's recording, this process becomes the app (exec keeps the
# pid). The kill then reaches the app, and perf, seeing it gone, finishes
# its file and exits.
#
# Environment (run.mjs sets it):
#   PROFILE_APP  the app's binary
#   PROFILE_OUT  where the recordings go: <scenario>-<n>.data, named for
#                the scenario in PROFILE_OUT/.scenario, and perf's pid in
#                <scenario>-<n>.pid until it has finished
#   PROFILE_CG   how perf walks the stack: fp (frame pointers, the default)
#                or dwarf (deeper through the system's libraries, ~60x the disk)
#   PROFILE_HZ   samples a second (default 999)
#   PERF         the perf command (default perf)
set -euo pipefail

out=${PROFILE_OUT:?}
app=${PROFILE_APP:?}
scenario=$(cat "$out/.scenario" 2>/dev/null || echo app)
n=1
while [ -e "$out/$scenario-$n.data" ] || [ -e "$out/$scenario-$n.pid" ]; do n=$((n + 1)); done
name="$out/$scenario-$n"
mkfifo "$name.ctl" "$name.ack"
# Started disabled (-D -1) and enabled through the control fifo, whose
# answer says perf is attached; -k mono stamps samples with the clock the
# harness times steps by (CLOCK_MONOTONIC), so a step's samples can be found.
setsid "${PERF:-perf}" record -z -F "${PROFILE_HZ:-999}" --call-graph "${PROFILE_CG:-fp}" -k mono \
  -p $$ -o "$name.data" -D -1 --control "fifo:$name.ctl,$name.ack" \
  > "$name.log" 2>&1 < /dev/null &
echo $! > "$name.pid"
# Both ends opened read-write, which doesn't wait for perf: a perf that
# stopped before opening them (a bad option) would otherwise hang the start.
exec 3<> "$name.ctl" 4<> "$name.ack"
echo enable >&3
reply=
read -r -t 20 reply <&4 || true
exec 3>&- 4<&-
rm -f "$name.ctl" "$name.ack"
# Without perf's answer the app still starts, unprofiled, and the log says why.
[ "$reply" = ack ] || echo "profile.sh: perf didn't start recording (see $name.log)" >&2
exec "$app" "$@"
