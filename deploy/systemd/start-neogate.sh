#!/usr/bin/env bash
set -u

mkdir -p /var/log/neogate

NEOGATE_BIN="${NEOGATE_BIN:-./target/release/neogate}"
NEOGATE_SCHEDULER_BIN="${NEOGATE_SCHEDULER_BIN:-./target/release/neogate-scheduler}"

if [ "${NEOGATE_SCHEDULER_LOG_STDOUT:-0}" = "1" ]; then
  "$NEOGATE_SCHEDULER_BIN" &
else
  "$NEOGATE_SCHEDULER_BIN" >>/var/log/neogate/scheduler.log 2>>/var/log/neogate/scheduler-error.log &
fi
scheduler_pid=$!

"$NEOGATE_BIN" &
backend_pid=$!

shutdown() {
  kill "$backend_pid" "$scheduler_pid" 2>/dev/null || true
}

trap shutdown TERM INT

wait -n "$backend_pid" "$scheduler_pid"
status=$?

shutdown
wait || true
exit "$status"
