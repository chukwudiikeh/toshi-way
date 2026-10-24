#!/usr/bin/env bash
# One command to bring up everything a demo needs, and to put it right again when something drifts.
#
#   ./scripts/demo-env.sh up        start the Lightning network, server, demo wallet and web app; seed the cast
#   ./scripts/demo-env.sh refresh   mine a block (LND reports "not synced" when the newest block is over 2 hours old)
#   ./scripts/demo-env.sh status    what is running, and each node's spendable balance
#   ./scripts/demo-env.sh down      stop the server, wallet and web app (the Lightning network keeps running)
#   ./scripts/demo-env.sh reset     down, then forget the demo database and cast so the next `up` starts clean
#
# Local regtest only. State lives in .demo/ (gitignored): the operator token, the key that encrypts
# sellers' node credentials, the database, logs, pids, and the seeded accounts with their tokens.
set -euo pipefail

cd "$(dirname "$0")/.."
ROOT=$(pwd)
DEMO="$ROOT/.demo"
DC="docker compose -f regtest/docker-compose.yml"
API=${API:-http://127.0.0.1:3000}
WEB=${WEB:-http://localhost:3001}
WALLET=${WALLET:-http://127.0.0.1:3100}
mkdir -p "$DEMO"

say()  { printf '\033[1m==> %s\033[0m\n' "$*"; }
warn() { printf '\033[33mwarning:\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[31merror:\033[0m %s\n' "$*" >&2; exit 1; }
up_http() { curl -fsS -o /dev/null --max-time 3 "$1" 2>/dev/null; }
btc() { $DC exec -T bitcoind bitcoin-cli -regtest -rpcuser=toshi -rpcpassword=toshi "$@"; }

wait_http() { # name url seconds
  for _ in $(seq 1 "$3"); do up_http "$2" && return 0; sleep 1; done
  die "$1 did not come up at $2 (see $DEMO/$1.log)"
}

stop_pid() { # name
  local f="$DEMO/$1.pid"
  [ -f "$f" ] || return 0
  local pid; pid=$(cat "$f")
  if kill -0 "$pid" 2>/dev/null; then
    pkill -P "$pid" 2>/dev/null || true   # children first (npm run dev spawns node)
    kill "$pid" 2>/dev/null || true
  fi
  rm -f "$f"
}

# Stop whatever is listening on a TCP port (used when a pid file was lost, e.g. .demo/ was deleted).
kill_port() {
  local pids
  pids=$(ss -ltnp "sport = :$1" 2>/dev/null | grep -o 'pid=[0-9]*' | cut -d= -f2 | sort -u || true)
  [ -n "$pids" ] && kill $pids 2>/dev/null || true
}

mine_one() {
  btc loadwallet miner >/dev/null 2>&1 || true
  btc -rpcwallet=miner -generate=1 >/dev/null
}

preflight() {
  for c in docker cargo node npm curl openssl python3 protoc; do
    command -v "$c" >/dev/null || die "$c is not installed"
  done
  docker info >/dev/null 2>&1 || die "docker is not running"
  [ "$(node -p 'process.versions.node.split(".")[0]')" -ge 20 ] || die "Node 20 or newer is needed"
  # Some container hosts route bridged traffic through a legacy firewall that drops it, so LND never
  # reaches bitcoind. Turning that off is harmless and lasts until the machine restarts.
  if [ -r /proc/sys/net/bridge/bridge-nf-call-iptables ] && [ "$(cat /proc/sys/net/bridge/bridge-nf-call-iptables)" = 1 ]; then
    if sudo -n sysctl -w net.bridge.bridge-nf-call-iptables=0 >/dev/null 2>&1; then
      say "set net.bridge.bridge-nf-call-iptables=0 so containers can reach each other"
    else
      warn "net.bridge.bridge-nf-call-iptables is 1; if LND never syncs run: sudo sysctl -w net.bridge.bridge-nf-call-iptables=0"
    fi
  fi
}

load_secrets() {
  [ -f "$DEMO/env" ] || {
    umask 077
    {
      echo "OPERATOR_TOKEN=$(openssl rand -hex 16)"
      echo "NODE_CREDENTIAL_KEY=$(openssl rand -hex 32)"
    } > "$DEMO/env"
  }
  # shellcheck disable=SC1091
  set -a; . "$DEMO/env"; set +a
}

start_lightning() {
  say "Lightning regtest network (bitcoind + buyer, router, seller, seller2)"
  if ! ./regtest/setup.sh > "$DEMO/regtest.log" 2>&1; then
    warn "setup did not finish; mining a block (stale chain is the usual cause) and retrying once"
    $DC up -d >/dev/null 2>&1 || true
    mine_one || true
    ./regtest/setup.sh > "$DEMO/regtest.log" 2>&1 || { tail -5 "$DEMO/regtest.log" >&2; die "the regtest network would not start (log: .demo/regtest.log)"; }
  fi
  mine_one   # always leave a fresh block so LND reports it is synced
}

start_server() {
  say "toshi-server on $API"
  if up_http "$API/health"; then say "already running"; return; fi
  cargo build -q -p escrow-api --bin toshi-server 2> "$DEMO/build.log" || { cat "$DEMO/build.log" >&2; die "build failed"; }
  # Each service gets its own redirected, detached shell and is exec'd, so the pid file holds the real
  # process and nothing keeps this script's output pipe open after it exits.
  # A room full of people behind one network address would trip the default 10 sign-ups a minute.
  ( exec env DATABASE_PATH="$DEMO/toshi.db" ALLOW_PRIVATE_NODE_ADDRESSES=1 REGISTER_LIMIT_PER_MIN=200 \
      ./target/debug/toshi-server ) \
    > "$DEMO/server.log" 2>&1 < /dev/null &
  echo $! > "$DEMO/server.pid"
  wait_http server "$API/health" 40
}

start_wallet() {
  say "demo wallet on $WALLET"
  if up_http "$WALLET/health"; then say "already running"; return; fi
  ( exec node scripts/demo-wallet.mjs ) > "$DEMO/wallet.log" 2>&1 < /dev/null &
  echo $! > "$DEMO/wallet.pid"
  wait_http wallet "$WALLET/health" 10
}

start_web() {
  say "web app on $WEB"
  if up_http "$WEB"; then say "already running (restart it with 'down' then 'up' if the demo wallet button is missing)"; return; fi
  [ -d web/node_modules ] || (cd web && npm install --no-audit --no-fund >/dev/null)
  ( cd web && exec env NEXT_PUBLIC_DEMO_WALLET_URL="$WALLET" npm run dev -- -p 3001 ) \
    > "$DEMO/web.log" 2>&1 < /dev/null &
  echo $! > "$DEMO/web.pid"
  wait_http web "$WEB" 60
}

balances() {
  for n in buyer seller seller2; do
    local sats
    sats=$($DC exec -T "$n" lncli --network=regtest channelbalance 2>/dev/null | python3 -c 'import sys,json; print(json.load(sys.stdin)["local_balance"]["sat"])' 2>/dev/null || echo "?")
    printf '  %-8s %s sats spendable\n' "$n" "$sats"
  done
}

summary() {
  cat <<EOS

Everything is up.

  App            $WEB
  API            $API   (health: $(curl -fsS "$API/health" 2>/dev/null || echo unreachable))
  Operator token $OPERATOR_TOKEN   (sign in at $WEB/operator)

Cast and tokens: $DEMO/accounts.json
  Arbiter key backup (paste on the Arbiter page > Restore): $DEMO/arbiter-key.json

Node balances:
$(balances)

Run the scripted rehearsal:  (cd web && npm run rehearse)
Before going on stage:       ./scripts/demo-env.sh refresh   (keeps LND synced)
EOS
}

cmd=${1:-}
case "$cmd" in
  up)
    preflight
    load_secrets
    start_lightning
    export OPERATOR_TOKEN NODE_CREDENTIAL_KEY
    start_server
    start_wallet
    start_web
    say "seeding the demo cast"
    API="$API" node scripts/seed-demo.mjs
    summary
    ;;
  refresh)
    mine_one
    say "mined a block"
    curl -fsS "$API/health" 2>/dev/null && echo || warn "server not reachable at $API"
    ;;
  status)
    for pair in "server:$API/health" "wallet:$WALLET/health" "web:$WEB"; do
      name=${pair%%:*}; url=${pair#*:}
      if up_http "$url"; then printf '  %-8s up    %s\n' "$name" "$url"; else printf '  %-8s DOWN  %s\n' "$name" "$url"; fi
    done
    echo "  health   $(curl -fsS --max-time 3 "$API/health" 2>/dev/null || echo unreachable)"
    echo "Node balances:"; balances
    ;;
  down)
    for p in web wallet server; do stop_pid "$p"; done
    # Anything still holding the demo's ports (a lost pid file) is stopped too.
    for port in 3001 3100 3000; do kill_port "$port"; done
    sleep 1
    say "stopped the server, demo wallet and web app (the Lightning network is still running)"
    ;;
  reset)
    "$0" down
    rm -f "$DEMO/toshi.db" "$DEMO/toshi.db-shm" "$DEMO/toshi.db-wal" "$DEMO/accounts.json" "$DEMO/arbiter-key.json"
    say "forgot the demo database and cast; run '$0 up' for a clean start"
    ;;
  *)
    sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'
    exit 2
    ;;
esac
