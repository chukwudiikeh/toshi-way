#!/usr/bin/env bash
# Fund an escrow in naira from its invoice, the way `demo-wallet` funds one in sats. Demo only.
#
#   ./scripts/pay-naira.sh <lnbcrt... invoice> [bank_code] [account_number]
#
# It finds the escrow the invoice belongs to and, as the operator, starts a naira payment for the
# escrow's buyer and marks the (pretend) bank transfer as arrived. The demo provider's node then pays
# the invoice and the escrow locks, exactly as if the buyer had pressed "Pay in naira" in the app.
# The bank account (default GTBank 0123456789) is where the naira would be refunded.
#
# Needs the stack from `./scripts/demo-env.sh up` (RAMP_PROVIDER=demo) on local regtest.
set -euo pipefail

cd "$(dirname "$0")/.."
API="${API:-http://127.0.0.1:3000}"
DC="docker compose -f regtest/docker-compose.yml"

invoice=$(printf '%s' "${1:-}" | tr -d '[:space:]' | tr '[:upper:]' '[:lower:]')
invoice=${invoice#lightning:}
bank="${2:-058}"
account="${3:-0123456789}"
[[ "$invoice" =~ ^lnbcrt[0-9a-z]{20,4000}$ ]] || { echo "usage: $0 <lnbcrt... invoice> [bank_code] [account_number]" >&2; exit 2; }
[ -f .demo/env ] || { echo "no .demo/env: start the demo with ./scripts/demo-env.sh up" >&2; exit 1; }
token=$(sed -n 's/^OPERATOR_TOKEN=//p' .demo/env)

call() { # method path [json]
  local out code
  out=$(curl -sS -X "$1" "$API$2" -H "Authorization: Bearer $token" -H 'content-type: application/json' ${3:+-d "$3"} -w '\n%{http_code}')
  code=${out##*$'\n'}
  out=${out%$'\n'*}
  if [ "$code" -ge 300 ]; then echo "$1 $2 failed ($code): $out" >&2; exit 1; fi
  printf '%s' "$out"
}

hash=$($DC exec -T buyer lncli --network=regtest decodepayreq "$invoice" | python3 -c 'import sys,json; print(json.load(sys.stdin)["payment_hash"])')
id=$(call GET /api/v1/operator/escrows | python3 -c "
import sys, json
hits = [e for e in json.load(sys.stdin)['escrows'] if e['payment_hash'] == '$hash']
print(hits[0]['id'] if hits else '')")
[ -n "$id" ] || { echo "no escrow in progress has this invoice (already paid, or from another server?)" >&2; exit 1; }

order=$(call POST "/api/v1/operator/escrows/$id/demo-onramp" "{\"bank_code\":\"$bank\",\"account_number\":\"$account\"}")
python3 -c "
import sys, json
o = json.loads(sys.argv[1]); x = 10 ** o['exponent']
print(f\"escrow {o['escrow_id']}: {o['amount_sats']:,} sats paid in naira\")
print(f\"  buyer pays  NGN {o['fiat_minor']/x:,.2f} (fee NGN {o['fee_minor']/x:,.2f}) to {o['deposit']['bank_name']}, ref {o['deposit']['reference']}\")
print(f\"  refunds to  {o['bank']['account_name']}, {o['bank']['bank_name']} {o['bank']['account_number']}\")" "$order"

for _ in $(seq 1 40); do
  state=$(call GET "/api/v1/escrows/$id" | python3 -c 'import sys,json; print(json.load(sys.stdin)["state"])')
  if [ "$state" = LOCKED ]; then echo "  escrow is LOCKED: funded"; exit 0; fi
  sleep 1
done
echo "  the transfer is recorded but the escrow is still $state; check .demo/server.log" >&2
exit 1
