# Flow log

A running record of what has been done on toshi-way, in order, and why. Newest entries go at the bottom. See [project.md](project.md) for the overall spec.

---

## 1. Reviewed the PRD

Read the HODL-invoice escrow PRD and gave feedback. Main points:

- Strengths: build the node integration first, node events are the source of truth, 256-bit preimage, and the refund demo.
- Concerns: the buyer holds all the power (they can withhold `P` and still get refunded), so delivery is the strongest use case. Requiring sellers to run a hold-invoice-capable node excludes most people, so the service may need to host the seller node. Hold times of "a day or two" are optimistic. The stack is heavy for a hackathon.
- Decision: do Phase 1 (prove the hold-invoice cycle) before any API or UI.

## 2. Chose a regtest setup without Polar

Polar is a desktop GUI around Docker containers and isn't available in this WSL environment. Checked the machine and found Docker 29.8.1, Docker Compose v5.5.1, Rust (cargo/rustc), Node and Python 3 already installed (no `jq`, so the scripts parse JSON with Python).

Decision: replace Polar with a plain `docker-compose.yml` plus a setup script, kept in the repo so the network is reproducible with one command.

## 3. Built the regtest network

Files added under `regtest/`:

- `docker-compose.yml`: one `bitcoind` (`lncm/bitcoind:v27.0`, regtest, ZMQ enabled) and three LND nodes (`lightninglabs/lnd:v0.18.5-beta`) named `buyer`, `router` and `seller`. gRPC is published on host ports 10001 / 10002 / 10003, REST on 8081 / 8082 / 8083. Each node has its own named volume.
- `setup.sh`: starts the containers, creates a miner wallet and mines 101 blocks, waits for each node to sync, funds each node with 1 BTC, opens channels `buyer → router` and `router → seller` (500,000 sats each, 100,000 pushed to the far side), waits for them to become active, and exports each node's `tls.cert` and `admin.macaroon` to `regtest/creds/<node>/`. Safe to re-run: it skips channels that already exist.
- `.gitignore`: excludes `regtest/creds/`, `target/`, `node_modules/` and `.env`.

One snag: my first compose file used YAML anchors to share the LND flags, but anchors can't splice a list into a `command`, so Compose rejected it. I generated the file with each node's flags written out in full instead.

## 4. Verified the network

- A normal 1000-sat invoice from `seller` was paid by `buyer` through `router`. Result: seller invoice `SETTLED`, buyer payment `SUCCEEDED` with a 1-sat routing fee. This proves the channels are open and the route works.
- `addholdinvoice` on `seller` for a random 32-byte preimage hash returned a payment request, and the invoice sits in `OPEN` state. This proves the hold-invoice RPC is available in this LND build.

Nothing here has been committed to git yet.

## 5. Wrote the docs

- `docs/project.md`: condensed project spec (concept, modes, state machine, architecture, API, milestones, demo script, regtest instructions, status).
- `docs/flow.md`: this file.

---

## Next

Phase 1 Rust program: create a hold invoice for a known hash on `seller`, pay it from `buyer`, observe the `ACCEPTED` state, then settle with the preimage. Repeat with cancel to confirm the buyer is refunded.
