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

## 6. Built and ran Phase 1 (`phase1/`)

A Cargo workspace (`Cargo.toml`) with one binary crate, `phase1`. Run it with `cargo run -p phase1 -- [settle|cancel|both]` after `./regtest/setup.sh`.

What it does: one program plays buyer and seller against the real LND nodes. The buyer side generates a random 32-byte `P` and `H = sha256(P)`. The seller node creates a hold invoice for `H` (1000 sats). The buyer node pays it through the router in a background task. The program watches the seller's invoice stream and waits for `ACCEPTED`, then either settles with `P` (after re-checking `sha256(P) == H`) or cancels. It then checks the payment status and channel balances.

Build notes:

- `tonic_lnd` 0.5.1 only exposes the lightning, wallet, signer and peers services, so it has no invoices or router. Switched to `fedimint-tonic-lnd` 0.4 with the `invoicesrpc` and `routerrpc` features.
- Its build needs `protoc`, which wasn't installed. Put protoc 28.3 in `~/.local/bin` (no sudo).
- The first run panicked because rustls had no crypto provider selected. Added `rustls` with the `ring` feature and installed it at startup.

### Results (both scenarios pass)

Settle path:

| Step | Seller invoice | Buyer payment | Buyer / seller balance |
|------|---------------|---------------|------------------------|
| before | - | - | 395528 / 101000 |
| paid, held | `OPEN` then `ACCEPTED` | in flight | 394097 / 101000 |
| settled with `P` | `SETTLED` | `Succeeded` | 394527 / 102000 |

Cancel path:

| Step | Seller invoice | Buyer payment | Buyer / seller balance |
|------|---------------|---------------|------------------------|
| before | - | - | 394527 / 102000 |
| paid, held | `OPEN` then `ACCEPTED` | in flight | 393096 / 102000 |
| cancelled | `CANCELED` | `Failed` (reason 4, incorrect payment details) | 394527 / 102000 |

### What this tells us

- The lock is real: while the invoice is `ACCEPTED` the buyer's balance has already dropped and the seller's has not moved. The money is committed but not spent.
- Settle works: the seller gains exactly 1000 sats and the buyer pays 1000 plus a 1-sat routing fee.
- Cancel refunds in full: the buyer's balance returns exactly to its starting value, fee included, and the seller is unchanged.
- The node's invoice stream emits states in the order `OPEN`, `ACCEPTED`, then `SETTLED` or `CANCELED`. The Lightning adapter and the escrow state machine should be driven by these events.
- A cancelled hold shows up on the buyer side as a failed payment with `FAILURE_REASON_INCORRECT_PAYMENT_DETAILS`. The core should treat that as the refund signal, not as an error.
- The balance seen by the buyer while held is about 430 sats lower than the final cost. Part of that is channel reserve and commitment fee accounting, not payment. Don't use the buyer's channel balance as proof of lock; use the invoice state.

Not yet tested: the timeout path where nobody acts and the HTLC expires on its own (needs mined blocks), hold behaviour across a node restart, and wrong-preimage handling.

---

## 7. Timeout path (`phase1 -- timeout`)

Added a `timeout` mode to `phase1`. It creates a hold invoice with a small `cltv_expiry` (24), pays it, waits for `ACCEPTED`, then takes **no action** and mines one block at a time (via `docker compose exec bitcoind bitcoin-cli -generate`) until the invoice changes state.

Result:

| Measurement | Value |
|-------------|-------|
| Invoice `cltv_expiry` requested | 24 blocks |
| Height when locked | 119 |
| HTLC `expiry_height` reported by the node | 146 (27 blocks out, so the payer adds about 3 blocks on top of the invoice's final delta) |
| Height when the node auto-cancelled | 134 (15 blocks after lock) |
| Margin before HTLC expiry at cancel | 12 blocks |
| Seller invoice | `OPEN`, `ACCEPTED`, `CANCELED` |
| Buyer payment | `Failed` (reason 4), same as an explicit cancel |
| Balances | buyer 394527 -> 394527, seller 102000 -> 102000 (full refund, seller untouched) |

### What this tells us

- The refund path needs no human action and no service action: if nobody settles, LND cancels the hold itself and the payment fails back. This is the proof the PRD asks for.
- LND cancels **12 blocks before the HTLC expires**. That is its hold-expiry margin (`invoices.holdexpirydelta`, default 12), not something we set. It protects the channel from a force-close.
- So the usable hold window is roughly `final_cltv_delta + ~3 - 12` blocks. With the default final delta of 80 that is about 71 blocks, or around 12 hours at ten minutes per block. A "day or two" hold is not achievable with defaults; the PRD's maximum hold time should be set from this.
- The service's own deadline must fire **before** LND's auto-cancel, i.e. earlier than `expiry_height - 12`, and the settle-by time must leave room for the seller to settle after receiving `P`. Suggested rule: `settle_by = expiry_height - 12 - safety_margin` (for example 6 more blocks), converted to wall-clock time conservatively.
- Cancel-by-timeout and cancel-by-request look identical to the buyer (failed payment, reason 4), so the core must use the invoice state, not the payment error, to know why it ended.
- Regtest blocks are mined on demand, so these are block counts, not times. Wall-clock hold times only mean something on a network with real block times.

Still untested: hold behaviour across a node restart, a wrong preimage, and a real wallet as the payer.

---

## 8. Phase 2, part 1: escrow core (`escrow-core/`)

A library crate with no web server, database or node dependency, so every rule can be tested in-process. `cargo test -p escrow-core` runs 27 tests (7 unit, 20 end-to-end against a mock node); all pass.

Modules:

- `state.rs`: the eight states and the allowed transitions from the PRD. `SETTLED`, `REFUNDED`, `EXPIRED_UNPAID` and `FAILED` have no exits; any non-terminal state may go to `FAILED`.
- `timing.rs`: turns a deadline into hold-window numbers using the Phase 1 measurements (LND cancels 12 blocks before expiry, payer adds about 3 blocks). `final_cltv_delta = hold_blocks + 12 + safety(6) - 3`, floored at 18. Rejects deadlines under 60 s or over the maximum hold (default 6 h).
- `model.rs`: `Escrow`, `AuditEntry`, node invoice types, and `Preimage`. `Preimage` only parses exactly 32 bytes (so a short code can never be a release secret) and its `Debug` output is redacted, so it cannot reach a log by accident. It is never part of `Escrow`.
- `adapter.rs`: the `LightningAdapter` trait (`create_hold_invoice`, `settle_invoice`, `cancel_invoice`, `lookup_invoice`). The real LND adapter and the mock both implement it.
- `mock.rs`: in-memory node that behaves as LND did in Phase 1 (`Open` -> `Accepted` -> `Settled`/`Canceled`), with switches to go offline, expire unpaid, or auto-cancel a held HTLC.
- `store.rs`: the `Store` trait with an in-memory implementation. `transition` is compare-and-set and writes the audit entry in the same step, so two actors (a node event and the scheduler) cannot both win. A SQL implementation is still to come.
- `service.rs`: `create`, `accept`, `apply_event`, `release`, `cancel`, `dispute`, `tick` (the scheduler pass) and `reconcile` (startup recovery).

Rules the service enforces:

- Payment state comes only from the node. `LOCKED`, `SETTLED`, `REFUNDED` and `EXPIRED_UNPAID` are reached via a node event or a node lookup, never because a caller said so. After `settle` or `cancel` the service re-reads the invoice from the node and follows it.
- `release` checks `sha256(P) == H` before the node is called. A wrong preimage changes nothing and makes no node call.
- Releases are refused once the deadline passes (`TooLate`), and the scheduler cancels the hold at the deadline, so our cancel fires before LND's own.
- Idempotency: repeating `accept`, `release`, `cancel` or `dispute` returns the existing result without a second node call or a second audit entry.
- A node that is offline at settle time leaves the escrow `LOCKED` and the call can be retried.
- A node state that should be impossible for the escrow's state (for example `SETTLED` on an unpaid invoice) sends the escrow to `FAILED` for operator review.
- A hash cannot be used by two escrows.

Failure cases from the PRD's test list now covered: wrong preimage, release twice, node offline at settle, restart mid-escrow (reconcile), payment never made, deadline reached while a dispute is open, and an action in the wrong state. Unauthorized access is not covered yet because authorization belongs to the API layer.

Decisions worth knowing:

- Single deadline: `deadline_at` is the settle-by time. The PRD's separate `settle_by_at` is not stored, because it would always be the same value.
- Timestamps are plain unix seconds passed in by the caller, which keeps time-dependent logic deterministic in tests.
- Block-time variance: the deadline is converted to blocks at 10 minutes per block. Blocks can arrive faster than that, so the 6-block safety margin is what keeps our cancel ahead of LND's.

Not done yet in Phase 2: the real LND adapter, a SQL store (SQLite or Postgres), the background loop that feeds node events into `apply_event` and runs `tick`, and wiring `reconcile` into startup.

---

## 9. Phase 2, part 2: SQLite store and LND adapter

Three more crates, so the workspace is now `phase1`, `escrow-core`, `escrow-sqlite` and `escrow-lnd`.

### Store contract and node events (`escrow-core`)

- Added `NodeEventRecord` and `Store::record_node_event` / `node_events`, matching the PRD's node-event table. `apply_event` records every event it handles (state plus a hash of the raw event) before acting on it.
- Added `escrow_core::testing::store_contract`, one test every `Store` backend must pass: round trip, duplicate insert, compare-and-set transitions (a stale transition fails and leaves no audit entry), update fields kept across empty updates, ordered audit log, active listing, node events. `MemoryStore` and SQLite both pass it. A future Postgres store only needs to implement the trait and pass the same function.

### SQLite store (`escrow-sqlite`)

- Tables: `escrows`, `audit_log`, `node_events` (schema in `escrow-sqlite/src/schema.sql`, created with `IF NOT EXISTS` on open). WAL mode, foreign keys on. Payment hash is stored as hex and has a `UNIQUE` index as a database-level backstop for the "one hash per escrow" rule.
- `transition` runs in one SQL transaction: `UPDATE ... WHERE id = ? AND state = ?` (compare-and-set), then the audit insert. If no row matched it reports `NotFound` or `Conflict`.
- There is deliberately no column for a preimage, and a test checks the schema text for it.
- Tests: the shared contract, a full flow that closes the database file, reopens it, reconciles against the node and settles, and the hash-uniqueness rule.
- Switching to Postgres later means a new `escrow-postgres` crate implementing `Store`; the service, adapter and tests do not change.
- No migration framework yet. The schema is create-if-missing; schema changes will need migrations before any real data matters.

### LND adapter (`escrow-lnd`)

- `LndAdapter` implements `LightningAdapter` over gRPC. Every call is wrapped in a timeout and failures map to `Unavailable` (retryable: unreachable, timed out, aborted) or `Rejected` (the node said no).
- `settle_invoice` and `cancel_invoice` are idempotent: if LND returns an error, the adapter looks the invoice up and treats "already settled" or "already cancelled" as success.
- `lookup_invoice` uses `LookupInvoiceV2` and returns the held HTLC's expiry height when one exists.
- `runner::run_event_loop` keeps a watcher per open escrow, feeds each state change into `apply_event`, reconciles at startup and every 30 s as a safety net, and resubscribes with backoff when a stream drops. `runner::run_scheduler` calls `tick` every second so deadlines fire without a human.

### Finding: the node-wide invoice stream does not work for this

My first event loop used LND's `SubscribeInvoices` (all invoices) and both service tests timed out waiting for `LOCKED`. A probe showed the subscribe call itself hangs until the first event, because LND sends no response headers on that stream until it has something to send, so the call timeout turned it into a permanent "unavailable". It is also documented to announce new and settled invoices, not a hold invoice moving to `ACCEPTED`.

Fix: subscribe per invoice with `SubscribeSingleInvoice`. LND sends the invoice's current state immediately, so a watcher that starts late still learns it was paid, and every state change arrives as a push. The periodic `reconcile` stays as a net underneath.

### Results against the real regtest network

`cargo test -p escrow-lnd -- --ignored --test-threads=1` (needs `./regtest/setup.sh` running; takes about a minute):

- `adapter_settle_cancel_and_idempotency`: create, `OPEN`, paid, `ACCEPTED` with an HTLC expiry height, settle (twice), then a second invoice cancelled (twice) with the buyer's payment failing back, and settling a cancelled invoice rejected.
- `service_settles_and_refunds_driven_by_live_node_events`: `LOCKED` reached from the live node event, wrong preimage rejected with state unchanged, correct preimage `SETTLED`, buyer payment `Succeeded`; a second escrow cancelled to `REFUNDED` with the payment failing back.
- `scheduler_refunds_automatically_at_the_deadline`: a 62-second escrow is paid and locked, then nothing is done by anyone; the scheduler cancels it at the deadline and it ends `REFUNDED` with the payment failed back. This is the refund demo from the PRD, running against real Lightning nodes.

Whole workspace: 33 normal tests plus the 3 network tests, all passing.

### Not done

- Restart-with-a-live-node test (the mock version exists in `escrow-core`; the real-node version is still to write).
- Restricted macaroon: tests use `admin.macaroon`. The service should bake one limited to invoice operations.
- Structured logging setup: the code emits `tracing` events but nothing installs a subscriber yet.
- Hold-time tuning against real wallets.

---

## 10. Phase 3: the API (`escrow-api/`)

An Axum server crate. The library builds a router that is generic over the node adapter and the store, so tests drive it in-process with the mock node; the `toshi-server` binary wires it to SQLite, the LND adapter, the event loop and the scheduler. 17 API tests, plus unit tests for auth and rate limiting.

### Core additions this needed

- `User`, `Role` (`user` or `operator`) and a `UserStore` trait with in-memory and SQLite implementations (new `users` table) and a shared contract test. Only the SHA-256 of a token is stored; a test checks the raw token is not a lookup key.
- `LightningAdapter::ping` (LND `GetInfo`, requires `synced_to_chain`) for `/health`.
- `EscrowService::list_active`, `node_events`, `node_health`.

### Authentication and authorization

- Accounts are created with `POST /api/v1/users`, which returns a random 256-bit token once. Requests use `Authorization: Bearer <token>`. The operator account is created at startup from `OPERATOR_TOKEN`.
- Buyer and seller are per escrow, not per account. The caller's standing on an escrow is worked out from the escrow itself.
- Someone with no standing on an escrow gets **404**, not 403, so escrow ids cannot be probed.
- Who may do what: only the buyer accepts and fetches the invoice; buyer or seller release and dispute; the operator reads everything, cancels, and sees the dispute queue but cannot release or dispute; operators cannot be a party to an escrow.
- **Cancel rule:** a buyer may cancel only before paying. After the funds are locked, cancelling hands the buyer a refund, so letting the buyer do it alone would strip the seller's protection. The seller (giving the money back) and the operator (resolving a dispute) can cancel a locked escrow.

### Endpoints

`POST /api/v1/users`, `POST /api/v1/escrows`, `GET /api/v1/escrows/{id}`, `POST .../accept`, `GET .../invoice`, `POST .../release`, `POST .../cancel`, `POST .../dispute`, `GET .../events`, `GET /api/v1/operator/escrows`, `GET /health`. Errors are always `{"error": {"code", "message"}}`.

### Safety properties (each has a test)

- The API only reports `LOCKED`, `SETTLED` or `REFUNDED` once the node has; the tests feed node events in exactly as the runner would.
- Release secrets must be exactly 64 hex characters. `123456`, empty, 31 bytes and non-hex are rejected with 400, a wrong 32-byte value with 422, and neither changes state or reaches the node.
- The preimage is never echoed back, never stored, and request bodies are not logged. The escrow view never includes the invoice (only the buyer's `/invoice` does, and only while it is payable).
- `Idempotency-Key` on create returns the first result instead of a second escrow; the same key with a different body is rejected (422); keys are per user. The other state-changing calls are idempotent by state (a repeated release returns the settled escrow with one node call).
- Rate limiting: 120 requests per minute per caller (token hash, or address when anonymous) and 10 registrations per minute per address, with `Retry-After`.
- Request bodies are capped at 16 KiB (413), requests time out at 30 s, and every response carries an `x-request-id` that is also propagated into logs.
- Node outage: calls return 503 `node_unavailable` and leave state untouched; `/health` reports `degraded` with 503.

### Live check against the regtest network (`scripts/demo.sh`)

With `toshi-server` running and `OPERATOR_TOKEN` set, the script runs both PRD demo paths over real HTTP with real Lightning payments:

1. Happy path: seller creates, buyer accepts and pays, state becomes `LOCKED` from the node's event, a wrong secret gets `wrong_preimage`, the right secret gets `SETTLED`.
2. Refund path: a 62-second escrow is locked and nobody acts; at the deadline it becomes `REFUNDED` automatically.

Run it: `OPERATOR_TOKEN=<24+ chars> ./target/debug/toshi-server &` then `OPERATOR_TOKEN=... ./scripts/demo.sh`. Also watched in the same session: an unpaid invoice is cancelled by the scheduler when it expires and leaves the operator queue.

### Things that went wrong on the way (worth knowing)

- **Stale regtest chain makes `/health` say degraded.** LND reports `synced_to_chain: false` when the newest block is over two hours old, and nothing mines on regtest unless asked. The health check is correct to report it, so I did not loosen it; `demo.sh` mines one block first.
- **Peer links drop when containers restart**, leaving channels inactive and payments failing with `INSUFFICIENT_BALANCE` / `NO_ROUTE`. `setup.sh` now re-establishes the peer connections on every run.
- An oversized body first returned 400 because my JSON wrapper flattened every rejection; it now keeps the real status (413).
- Two stale held HTLCs (2000 sats) remain from the earlier broken-event-loop test runs; they are harmless and LND will cancel them as blocks are mined.

### Known limitations

- **Buyers are invited by id.** The seller names the buyer's user id when creating the escrow. A shareable open-offer link where whoever opens it becomes the buyer would need the core model to allow an unset buyer.
- The idempotency cache lives in memory, so it is lost on restart (the escrow itself is not).
- Rate limiting is in-process and keyed by direct peer address; behind a reverse proxy it would need the forwarded address.
- The server uses `admin.macaroon` by default; a macaroon limited to invoice operations should replace it.
- No CORS configuration yet, which the web frontend will need.

---

## Next

Phase 4: the web frontend (Next.js): buyer and seller views with lock status, countdown, and a QR release. Before that, decide how the buyer hands the secret to the seller (QR of the 64-hex secret) and add CORS to the API.
