#!/bin/bash
set -eu
SIMDIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MODE="${1:-clean}"  # clean or faulted

if [ ! -f "$SIMDIR/host_key" ]; then
  ssh-keygen -t ed25519 -N "" -f "$SIMDIR/host_key" -q
fi

if [ -f "$SIMDIR/sshd.pid" ]; then
  OLDPID=$(cat "$SIMDIR/sshd.pid" 2>/dev/null || true)
  if [ -n "$OLDPID" ] && kill -0 "$OLDPID" 2>/dev/null; then
    kill "$OLDPID"
    sleep 0.3
  fi
  rm -f "$SIMDIR/sshd.pid"
fi

cat > "$SIMDIR/sshd_config" <<EOF
Port 2847
ListenAddress 127.0.0.1
HostKey "$SIMDIR/host_key"
AuthorizedKeysFile "$SIMDIR/authorized_keys_$MODE"
PasswordAuthentication no
PubkeyAuthentication yes
UsePAM no
StrictModes no
PidFile "$SIMDIR/sshd.pid"
LogLevel ERROR
EOF

/usr/sbin/sshd -f "$SIMDIR/sshd_config"
sleep 0.5
cat "$SIMDIR/sshd.pid"
