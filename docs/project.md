# toshi-way

Lightning HODL-invoice escrow. Two parties who don't trust each other transact over Bitcoin Lightning, with the payment held in escrow until a condition is met. Funds are released to the seller when the buyer (or recipient) reveals a secret, and refunded automatically if that doesn't happen before a deadline.

**Principle:** prove one real lock → condition → settle-or-refund cycle on Lightning before building anything else.

## How it works

A HODL invoice lets the receiving node accept a locked payment (HTLC) without revealing the preimage `P`. While it waits, the funds are committed but not spent. The node can then:

- **Settle** by revealing `P`, which pays the seller, or
- **Cancel**, which fails the HTLCs and refunds the buyer.

If the node does nothing before HTLC expiry, the lock times out and the buyer is refunded. That window is the escrow.

### Mode A: preimage release (MVP, non-custodial)

1. Buyer's client generates a random 32-byte secret `P` and `H = sha256(P)`. Only `H` is shared.
2. Seller's node creates a hold invoice for `H` and the agreed amount.
3. Buyer pays. The payment locks and shows as accepted-but-unsettled.
4. Seller sees funds are committed and does the job.
5. Buyer or recipient hands `P` to the seller (QR or long token).
6. Seller's node settles with `P`. If `P` never arrives, the invoice is cancelled or times out and the buyer is refunded.

`P` must be a full 256-bit value, never a short PIN, or the seller could brute-force it from `H`.

### Mode B: coordinator-held release (stretch)

The service runs the node and holds `P`, settling or cancelling based on a verified condition. Easier for users without a node, but custodial, so it must be labelled as such.

## Goals (MVP)

- Create an escrow with amount (sats), deadline and description.
- Lock the buyer's payment with a real hold invoice on regtest.
- Settle on correct `P`; refund automatically after the deadline.
- Drive escrow state from node events only, never from client claims.
- Web UI showing lock, countdown, release action and outcome.
- Demo both the happy path and the timeout-refund path.

## Non-goals (MVP)

Custody beyond what the node holds in flight, automated arbitration, fiat on/off-ramps, custom routing, holds longer than a day or two, custodial wallets as the seller side, tokens, native mobile apps, and proof that the real-world work happened.

## Escrow state machine

States: `CREATED`, `INVOICE_ISSUED`, `LOCKED`, `SETTLED`, `REFUNDED`, `EXPIRED_UNPAID`, `DISPUTED`, `FAILED`.

| From | To |
|------|----|
| CREATED | INVOICE_ISSUED, EXPIRED_UNPAID (offer withdrawn, declined or lapsed) |
| INVOICE_ISSUED | LOCKED, EXPIRED_UNPAID |
| LOCKED | SETTLED, REFUNDED, DISPUTED |
| DISPUTED | REFUNDED, SETTLED (operator, within safe window) |
| any | FAILED (unrecoverable error) |

`SETTLED` and `REFUNDED` are terminal. If escrow state and node state disagree, the node is authoritative.

## Architecture

- **Backend:** Rust, Axum, Tokio.
- **Node:** LND over gRPC (invoices and router services), behind a `LightningAdapter` trait so core logic is testable with a mock. A `NodeDirectory` picks the node for each seller: their own, or (if the operator allows it) the service's shared node.
- **Database:** SQLx behind the `Store` trait (escrows, node events, audit log, users): SQLite by default (`escrow-sqlite`) or PostgreSQL (`escrow-postgres`, selected by `DATABASE_URL`). Both have versioned migrations and pass the same store contract tests.
- **Frontend:** Next.js, TypeScript, Tailwind, Zod.
- **Modules:** web client, API layer, escrow core (state machine, idempotency), Lightning adapter, persistence, deadline scheduler.

Constraints: all node calls have timeouts; invoice subscriptions reconnect and reconcile on startup; state changes are transactional; restricted macaroons kept out of source control; `P` is never stored or logged.

## Timeouts and safety

Timeouts are the hardest part. The service enforces its own settle-by time well before real HTLC expiry (and before LND's hold-expiry margin), unpaid invoices expire in minutes, final CLTV delta is set deliberately, and creation rejects deadlines beyond a configured maximum hold. Exact values must be tested on regtest and recorded here.

Measured on regtest: LND auto-cancels a held HTLC 12 blocks before it expires (`invoices.holdexpirydelta`), and the payer adds about 3 blocks to the invoice's final CLTV delta. Usable hold window is therefore about `final_cltv_delta + 3 - 12` blocks, roughly 71 blocks (about 12 hours) with the default delta of 80. The service's settle-by must be earlier than `expiry_height - 12`.

## Whose node, and who holds the money

Each escrow records which node holds its invoice (`seller_node` in the API):

- **`own`** (non-custodial). The seller connected their own LND node. The hold invoice is created on it, so a release pays the seller directly and the service never holds the sats. Sellers supply the node's gRPC address, its TLS certificate and an invoice-only macaroon (`invoices:read invoices:write info:read`). Saving runs a check (connect, synced, create and cancel a probe invoice) and stores nothing if it fails. The macaroon is encrypted at rest (AES-256-GCM under `NODE_CREDENTIAL_KEY`), the connection is pinned to the supplied certificate, and the address must be public (no loopback, private, link-local or metadata addresses) unless `ALLOW_PRIVATE_NODE_ADDRESSES=1` for regtest.
- **`shared`** (custodial after release). The service's own node, used for sellers who have none when `SHARED_NODE` is on (the default). The operator receives the sats and owes them to the seller. The UI says so. Set `SHARED_NODE=off` so every seller must bring their own and the service can never receive anyone's money.

The kind is fixed when the escrow is created, so changing node settings cannot strand an invoice. A seller cannot change or remove their node while an escrow with an invoice is in flight on it.

## Trust model

In Mode A nobody holds the funds, but the seller's node sees `P` at settlement and the coordinator is trusted to relay state correctly. Revealing `P` shows the buyer chose to release, not that the work was good. The buyer holds the power to withhold `P`, so the strongest use case is delivery, where the recipient must be present at handoff.

## Planned API

| Method | Endpoint | Purpose |
|--------|----------|---------|
| POST | `/api/v1/escrows` | Create an escrow offer (`buyer_id` optional: omit it for an open offer) |
| GET | `/api/v1/escrows/:id` | Details and state |
| POST | `/api/v1/escrows/:id/accept` | Buyer accepts and submits `H` |
| GET | `/api/v1/escrows/:id/invoice` | Hold invoice to pay |
| POST | `/api/v1/escrows/:id/release` | Submit `P` to settle |
| POST | `/api/v1/escrows/:id/cancel` | Request cancellation |
| POST | `/api/v1/escrows/:id/dispute` | Flag a dispute |
| GET | `/api/v1/escrows/:id/events` | Audit and event history |
| GET | `/api/v1/me` | The caller's `{id, role}` |
| GET / PUT / DELETE | `/api/v1/me/node` | Read, connect (address, TLS cert, invoice macaroon) or remove the seller's own Lightning node |
| GET | `/health` | Health, including node connectivity and `server_time` |

## Milestones

1. **Prove the primitive.** From a script or minimal Rust program: create a hold invoice, pay it, observe accepted, settle; repeat with cancel. No UI until this works.
2. **Escrow core.** State machine, schema, idempotency, event-driven transitions, deadline scheduler.
3. **API.** Endpoints, auth, audit log, restart reconciliation.
4. **Frontend.** Buyer and seller views, QR release, outcome display.
5. **Hardening and demo.** Failure tests, timeout tuning, docs, rehearsed demo.

## Demo script

- **Happy path:** seller creates an offer, buyer accepts and pays, UI shows funds locked, buyer shows the release QR, seller scans it, payment settles.
- **Refund path:** a second escrow with a short deadline expires and refunds automatically with no human action. This proves it is a real escrow.

## Local regtest network

Defined in `regtest/`, run with Docker Compose (no Polar needed).

```bash
./regtest/setup.sh
```

This starts `bitcoind` plus four LND v0.18.5 nodes, funds them, and opens channels `buyer → router → seller` and `router → seller2` (`seller2` plays a second seller with their own node). It is safe to re-run. Credentials are exported to `regtest/creds/<node>/` (`tls.cert`, `admin.macaroon`), which is gitignored.

| Node | gRPC | REST |
|------|------|------|
| buyer | `localhost:10001` | `localhost:8081` |
| router | `localhost:10002` | `localhost:8082` |
| seller | `localhost:10003` | `localhost:8083` |
| seller2 | `localhost:10004` | `localhost:8084` |

Useful commands:

```bash
docker compose -f regtest/docker-compose.yml exec seller lncli --network=regtest getinfo
docker compose -f regtest/docker-compose.yml down        # stop, keep state
docker compose -f regtest/docker-compose.yml down -v     # stop and wipe the network
```

`admin.macaroon` is for local Phase 1 work only. `setup.sh` also bakes `toshi.macaroon` for the seller node, limited to `invoices:read`, `invoices:write` and `info:read`; `toshi-server` and the seller adapter in the tests use it.

## Status

- [x] PRD drafted
- [x] Regtest network: 3 LND nodes, channels open, routed payment verified, `addholdinvoice` verified
- [x] Phase 1 Rust program (`phase1/`): hold invoice → pay → accepted → settle / cancel, both verified
- [x] Timeout path: with no action LND auto-cancels 12 blocks before HTLC expiry and the buyer is refunded in full (see `flow.md` section 7)
- [x] Escrow core (`escrow-core/`): state machine, timing, `LightningAdapter` + mock, `Store` + in-memory store, service logic; 27 tests passing
- [x] Phase 2 part 2: SQLite store (`escrow-sqlite/`), LND adapter plus event and scheduler loops (`escrow-lnd/`); verified on regtest including the automatic deadline refund
- [x] Phase 3 API (`escrow-api/`): auth, per-escrow authorization, idempotency, rate limiting, `/health`; 20 tests; both demo paths verified over HTTP on regtest (`scripts/demo.sh`)
- [x] CORS, `GET /api/v1/escrows` (own escrows), Next.js scaffold in `web/`
- [x] Phase 4 frontend (`web/`): account, new escrow, escrow page by role and state with release QR hand-over, operator queue; 44 unit tests; both demo paths pass end to end in the browser (Playwright, `npm run e2e`)
- [x] `server_time` on `/health` for client clock-skew correction
- [x] Hardening 1: `CREATED` offers can be withdrawn (seller, operator) or declined (buyer) and lapse at their deadline; `GET /api/v1/me`
- [x] Hardening 2: restricted invoice-only macaroon (`regtest/creds/seller/toshi.macaroon`, now the server default), restart test on the real node
- [x] Hardening 3: SQLite schema migrations (`PRAGMA user_version`), open-offer links (no buyer id needed; the first user to accept becomes the buyer)
- [x] Extras: WebLN one-click payment, camera scan of the release QR, PostgreSQL store (`DATABASE_URL`)
- [x] Sellers bring their own node (non-goal lifted): per-seller node directory, encrypted credentials, `/api/v1/me/node`, "My node" page, second regtest seller node
- [ ] Demo rehearsal and choosing the lead use case

## Open questions

- Which use case leads the demo: courier delivery, freelance work, or P2P trade?
- ~~Who owns the seller's node?~~ Answered: the seller can connect their own (non-custodial); the shared hosted node is optional and labelled custodial.
- Regtest only, or signet/testnet with real wallets?
- Maximum supported hold time on the target network and node settings.

## Running the tests

```bash
cargo test --workspace                                    # fast, no network needed
./regtest/setup.sh                                        # once, to start the regtest network
cargo test -p escrow-lnd -- --ignored --test-threads=1    # real-node tests (4), about a minute

docker compose -f regtest/docker-compose.yml --profile postgres up -d postgres
cargo test -p escrow-postgres -- --ignored --test-threads=1   # store contract + race on a real PostgreSQL (wipes it)
DATABASE_URL=postgres://toshi:toshi@127.0.0.1:55432/toshi cargo run -p escrow-api --bin toshi-server   # run on PostgreSQL
cargo run -p phase1 -- both                               # the original hold-invoice demo

export OPERATOR_TOKEN=$(openssl rand -hex 16)             # 24+ characters
export NODE_CREDENTIAL_KEY=$(openssl rand -hex 32)        # keep it: it decrypts sellers' stored macaroons
export ALLOW_PRIVATE_NODE_ADDRESSES=1                     # regtest only: lets sellers point at localhost nodes
cargo run -p escrow-api --bin toshi-server &              # API on 127.0.0.1:3000, SQLite in ./toshi.db
./scripts/demo.sh                                         # both demo paths over HTTP
```

Building needs `protoc` on the PATH (the LND client generates code from protobuf files).
