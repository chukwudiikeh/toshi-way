#!/usr/bin/env bash
# Plays both PRD demo paths against a running toshi-server and the regtest network:
#   1. happy path: lock, release with the secret, settle
#   2. refund path: lock, do nothing, watch the automatic refund
# Usage: OPERATOR_TOKEN=... ./scripts/demo.sh   (server on $API, default http://127.0.0.1:3000)
set -euo pipefail

API=${API:-http://127.0.0.1:3000}
OPERATOR_TOKEN=${OPERATOR_TOKEN:?set OPERATOR_TOKEN to the server\'s operator token}
cd "$(dirname "$0")/.."
DC="docker compose -f regtest/docker-compose.yml"

# jget <expr>: read JSON on stdin, print an expression of it, e.g. jget '["id"]'
jget() { python3 -c "import sys,json; d=json.load(sys.stdin); print(d$1)"; }
# api <METHOD> <path> <token> [json-body]
api() {
  local method=$1 path=$2 token=$3 body=${4:-}
  if [ -n "$body" ]; then
    curl -sS -X "$method" "$API$path" -H "Authorization: Bearer $token" -H 'Content-Type: application/json' -d "$body"
  else
    curl -sS -X "$method" "$API$path" -H "Authorization: Bearer $token"
  fi
}
state() { api GET "/api/v1/escrows/$1" "$2" | jget '["state"]'; }
wait_for_state() { # id token wanted max_seconds
  for _ in $(seq 1 "$(($4 * 2))"); do
    [ "$(state "$1" "$2")" = "$3" ] && return 0
    sleep 0.5
  done
  echo "timed out waiting for $3 (state is $(state "$1" "$2"))" >&2; exit 1
}

# Regtest only mines on demand. LND reports "not synced" when the newest block is over two hours old,
# which /health surfaces as degraded, so make sure the chain tip is fresh.
BTC="$DC exec -T bitcoind bitcoin-cli -regtest -rpcuser=toshi -rpcpassword=toshi"
$BTC loadwallet miner >/dev/null 2>&1 || true
$BTC -rpcwallet=miner -generate=1 >/dev/null
for _ in $(seq 1 20); do curl -sSf "$API/health" >/dev/null 2>&1 && break; sleep 0.5; done

echo "== health"; curl -sS "$API/health"; echo

echo "== registering a seller and a buyer"
SELLER=$(curl -sS -X POST "$API/api/v1/users"); BUYER=$(curl -sS -X POST "$API/api/v1/users")
SELLER_TOKEN=$(echo "$SELLER" | jget '["token"]'); BUYER_TOKEN=$(echo "$BUYER" | jget '["token"]')
BUYER_ID=$(echo "$BUYER" | jget '["id"]')

new_escrow() { # hold_secs -> sets ID P H
  ID=$(api POST /api/v1/escrows "$SELLER_TOKEN" \
    "{\"buyer_id\":\"$BUYER_ID\",\"amount_sats\":1000,\"description\":\"demo parcel\",\"hold_secs\":$1}" | jget '["id"]')
  # The buyer's side makes the secret; only the hash is ever sent to the service.
  P=$(openssl rand -hex 32); H=$(echo -n "$P" | xxd -r -p | sha256sum | cut -d' ' -f1)
  api POST "/api/v1/escrows/$ID/accept" "$BUYER_TOKEN" "{\"payment_hash\":\"$H\"}" >/dev/null
  INVOICE=$(api GET "/api/v1/escrows/$ID/invoice" "$BUYER_TOKEN" | jget '["payment_request"]')
  echo "   escrow $ID  state=$(state "$ID" "$BUYER_TOKEN")"
  # The payment stays in flight while the funds are held.
  $DC exec -T buyer lncli --network=regtest payinvoice --force "$INVOICE" >/tmp/toshi-demo-pay.log 2>&1 &
  PAY_PID=$!
}

echo; echo "== PATH 1: happy path"
new_escrow 1800
wait_for_state "$ID" "$SELLER_TOKEN" LOCKED 20
echo "   funds are LOCKED (the node reported the invoice accepted)"
echo "   releasing with a wrong secret:"
api POST "/api/v1/escrows/$ID/release" "$SELLER_TOKEN" "{\"preimage\":\"$(openssl rand -hex 32)\"}"; echo
echo "   releasing with the buyer's secret:"
api POST "/api/v1/escrows/$ID/release" "$SELLER_TOKEN" "{\"preimage\":\"$P\"}" | jget '["state"]'
wait "$PAY_PID" || true
echo "   history:"; api GET "/api/v1/escrows/$ID/events" "$BUYER_TOKEN" | jget '["audit"]' | tr -d '\n' | sed 's/}, /},\n     /g'; echo

echo; echo "== PATH 2: refund path (62 second hold, nobody acts)"
new_escrow 62
wait_for_state "$ID" "$SELLER_TOKEN" LOCKED 20
echo "   LOCKED at $(date +%T); waiting for the deadline..."
wait_for_state "$ID" "$SELLER_TOKEN" REFUNDED 90
echo "   REFUNDED at $(date +%T) with no human action"
wait "$PAY_PID" 2>/dev/null || true
echo "   history:"; api GET "/api/v1/escrows/$ID/events" "$BUYER_TOKEN" | jget '["audit"]' | tr -d '\n' | sed 's/}, /},\n     /g'; echo

echo; echo "== operator queue (active escrows)"; api GET /api/v1/operator/escrows "$OPERATOR_TOKEN"; echo
