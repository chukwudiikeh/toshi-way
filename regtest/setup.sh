#!/usr/bin/env bash
# Bring up the regtest network, fund the nodes and open buyer -> router -> seller channels.
# Idempotent: safe to re-run; skips channels that already exist.
set -euo pipefail

cd "$(dirname "$0")"
DC="docker compose -f docker-compose.yml"
CREDS="$(pwd)/creds"
NODES=(buyer router seller)

btc()  { $DC exec -T bitcoind bitcoin-cli -regtest -rpcuser=toshi -rpcpassword=toshi "$@"; }
lncli() { local n=$1; shift; $DC exec -T "$n" lncli --network=regtest "$@"; }
jget() { python3 -c "import sys,json; d=json.load(sys.stdin); print(d$1)"; }
mine() { btc -rpcwallet=miner generatetoaddress "$1" "$(btc -rpcwallet=miner getnewaddress)" >/dev/null; }

wait_for() { # description, command...
  local what=$1; shift
  for _ in $(seq 1 60); do "$@" >/dev/null 2>&1 && return 0; sleep 1; done
  echo "timed out waiting for $what" >&2; exit 1
}

echo "==> starting containers"
$DC up -d

echo "==> waiting for bitcoind"
wait_for bitcoind btc getblockchaininfo
btc createwallet miner >/dev/null 2>&1 || btc loadwallet miner >/dev/null 2>&1 || true
if [ "$(btc getblockcount)" -lt 101 ]; then mine 101; fi

for n in "${NODES[@]}"; do
  echo "==> waiting for $n"
  wait_for "$n wallet" lncli "$n" getinfo
  wait_for "$n chain sync" bash -c "$DC exec -T $n lncli --network=regtest getinfo | python3 -c 'import sys,json; sys.exit(0 if json.load(sys.stdin)[\"synced_to_chain\"] else 1)'"
done

echo "==> funding wallets"
for n in "${NODES[@]}"; do
  bal=$(lncli "$n" walletbalance | jget "['confirmed_balance']")
  if [ "$bal" -lt 1000000 ]; then
    addr=$(lncli "$n" newaddress p2wkh | jget "['address']")
    btc -rpcwallet=miner sendtoaddress "$addr" 1 >/dev/null
  fi
done
mine 6
for n in "${NODES[@]}"; do
  wait_for "$n funds" bash -c "[ \$($DC exec -T $n lncli --network=regtest walletbalance | python3 -c 'import sys,json; print(json.load(sys.stdin)[\"confirmed_balance\"])') -ge 1000000 ]"
done

pubkey() { lncli "$1" getinfo | jget "['identity_pubkey']"; }

connect() { # from, to
  lncli "$1" connect "$(pubkey "$2")@$2:9735" >/dev/null 2>&1 || true
}

open_channel() { # from, to
  local from=$1 to=$2 pk
  pk=$(pubkey "$to")
  if lncli "$from" listchannels --peer "$pk" | jget "['channels']" | grep -q "chan_id"; then
    echo "    $from -> $to already open"; return
  fi
  connect "$from" "$to"
  echo "    opening $from -> $to"
  lncli "$from" openchannel --node_key "$pk" --local_amt 500000 --push_amt 100000 >/dev/null
  mine 6
}

echo "==> opening channels"
open_channel buyer router
open_channel router seller

# Peer links drop when containers restart; channels stay inactive until they are re-established.
echo "==> (re)connecting peers"
connect buyer router
connect router seller

echo "==> waiting for channels to become active"
for n in "${NODES[@]}"; do
  wait_for "$n channels" bash -c "$DC exec -T $n lncli --network=regtest getinfo | python3 -c 'import sys,json; sys.exit(0 if json.load(sys.stdin)[\"num_active_channels\"]>=1 else 1)'"
done
# Let gossip propagate so buyer can route to seller.
sleep 5

echo "==> exporting credentials to $CREDS (gitignored)"
for n in "${NODES[@]}"; do
  mkdir -p "$CREDS/$n"
  cid=$($DC ps -q "$n")
  docker cp "$cid:/root/.lnd/tls.cert" "$CREDS/$n/tls.cert"
  docker cp "$cid:/root/.lnd/data/chain/bitcoin/regtest/admin.macaroon" "$CREDS/$n/admin.macaroon"
  chmod 644 "$CREDS/$n/"*
done

# The service only ever needs invoice operations (add, settle, cancel, look up, subscribe) and
# GetInfo for /health. Bake a macaroon limited to exactly that for the seller node it talks to.
echo "==> baking the restricted service macaroon (seller)"
$DC exec -T seller sh -c 'rm -f /tmp/toshi.macaroon; lncli --network=regtest bakemacaroon invoices:read invoices:write info:read --save_to=/tmp/toshi.macaroon >/dev/null'
docker cp "$($DC ps -q seller):/tmp/toshi.macaroon" "$CREDS/seller/toshi.macaroon"
chmod 644 "$CREDS/seller/toshi.macaroon"

echo "==> done"
for n in "${NODES[@]}"; do
  echo "  $n  $(pubkey "$n")  channels=$(lncli "$n" getinfo | jget "['num_active_channels']")"
done
echo "gRPC: buyer localhost:10001, router localhost:10002, seller localhost:10003"
