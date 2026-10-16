#!/bin/bash
SIMDIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [ -f "$SIMDIR/sshd.pid" ]; then
  PID=$(cat "$SIMDIR/sshd.pid" 2>/dev/null || true)
  if [ -n "$PID" ] && kill -0 "$PID" 2>/dev/null; then
    kill "$PID"
    echo "stopped $PID"
  fi
  rm -f "$SIMDIR/sshd.pid"
fi
