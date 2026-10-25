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

## 11. Phase 4, step 1: CORS, a listing endpoint, and the frontend scaffold

Preparing the API for a browser app, and starting `web/`.

- **CORS** (`escrow-api`): `ApiConfig.cors_origins` is an allow-list of browser origins, set with `CORS_ORIGINS` (default `http://localhost:3001,http://127.0.0.1:3001`; the API is on 3000, so the frontend runs on 3001). The layer is outermost, so preflight `OPTIONS` requests are answered before authentication and rate limiting. Allowed headers: `Authorization`, `Content-Type`, `Idempotency-Key`; exposed to the page: `x-request-id`, `Retry-After`. A test checks an allowed origin gets the headers and an unlisted one gets none.
- **`GET /api/v1/escrows`**: the caller's own escrows as buyer or seller, newest first, at most 100. Without it a user had no way to find an escrow they had been invited to. Needed a new `Store::list_for_user` (in-memory and SQLite, with indexes on buyer and seller, covered by the shared store contract test). Operators are never parties, so they still use `/api/v1/operator/escrows`.
- **`invoice_expires_at`** is now in the escrow view so the UI can count down an unpaid invoice. The invoice itself is still only returned by `/invoice`, to the buyer.
- **Scaffold**: `web/` created with `create-next-app` (Next.js 16, React 19, Tailwind 4, TypeScript, ESLint, App Router). It is still the stock template; no feature code yet. `node_modules` and `.next` are gitignored.

Workspace tests: 61 passing (20 API, 23 core flows, 7 core unit, 5 SQLite, 3 API unit) plus the 3 real-node tests, which are ignored by default.

Frontend design decisions made before writing it:

- The secret `P` is generated in the browser (`crypto.getRandomValues`) and kept in that browser's local storage, keyed by escrow id. Only `H = sha256(P)` is sent to the server.
- The buyer hands `P` to the seller as a QR code of a link with the secret in the URL fragment (`/escrows/<id>#release=<64 hex>`). Fragments are never sent to a server, so the secret does not reach logs or the API until the seller presses release.
- The browser never decides payment state: it renders whatever state the API reports and polls for changes.
- Auth is the account token in local storage. That is simple and fine for the hackathon; the cost is that anything running on the page can read it, which is why the app avoids third-party scripts.

---

## 12. Phase 4, step 2: the web app

The screens from the brief, in `web/`: home/account, new escrow (seller), the escrow page by role and state, and the operator queue.

- **Structure.** `lib/api.ts` is the only place that talks to the API: bearer token, Zod-parsed responses, the `{error: {code, message}}` format, and 401 handling (a 401 on the session's token signs out). `lib/session.ts` is the only place that touches the token in `localStorage`. `lib/secret.ts` generates `P`, hashes it, validates 64-hex input and builds/parses `#release=` links. Pages are thin server components; the screens are client components because everything depends on the token in the browser.
- **Secret handling.** `P` is 32 bytes from `crypto.getRandomValues`, stored under `toshi.secret.<escrow id>` **before** the accept call goes out, so a lost response can't lose it. Only `sha256(P)` is sent. The buyer's page checks the stored secret against the escrow's `payment_hash` before showing a release QR, and warns if it is missing or doesn't match. The QR encodes `/escrows/<id>#release=<P>` and is hidden until the buyer confirms they have the goods. The escrow page takes the fragment, removes it with `history.replaceState`, keeps it in memory only, and waits for a button press. QR codes are generated locally (`qrcode.react`). No third-party scripts or fonts (the template's Google font was removed).
- **State comes from the server.** Every action renders the escrow the API returns; the page polls every 2 s until a final state. `invalid_state` and `too_late` trigger a refetch instead of an error. Network failures keep the last known state on screen with a "reconnecting" note and polling continues.
- **Clock skew (brief section 5).** `/health` now returns `server_time` (one line in `escrow-api/src/routes.rs`, asserted in the health test). The client measures an offset once per page load and runs countdowns on server time; they are labelled as indicative.
- **Sign-in detects the role.** The API has no "who am I" call. A pasted token is tried against `/api/v1/operator/escrows`: 200 means operator, 403 means user. A user's own id is learned from their first escrow (or typed in), since nothing else returns it.

Tests: 44 unit tests (Vitest: secret generation and the `sha256` vector, 63/65-char/short/non-hex rejection, link round trip, schemas against the documented sample, form limits, API client error handling and 401 sign-out). `e2e/escrow.spec.ts` (Playwright, system Chrome) runs both demo paths in two browser profiles against the live regtest stack, paying with `lncli` from the buyer node. Both pass:

1. Happy path: create → accept → pay → both see LOCKED → wrong secret refused (state unchanged) → 6-digit code refused client-side → buyer reveals QR → seller opens the release link → SETTLED on both screens. The test records every request from both browsers and asserts the secret appears in exactly one: the seller's `/release`. A third account opening the escrow gets "not found". The buyer has no cancel button while locked.
2. Refund path: a 62-second hold, nobody acts, both screens flip to REFUNDED on their own.

Checked by hand as well: stopping `toshi-server` mid-escrow shows "can't reach the server" over the last known state; restarting it clears the warning and the page follows the next state change. The operator can sign in, sees disputes first, and can cancel from the queue (checked at phone width). Neither the server log nor the Next.js log contains any 64-hex string, and no request carried `release=`.

### Things that went wrong on the way

- **A release link opened in a tab already showing that escrow did nothing.** Only the fragment changed, so there was no page load and the mount-time reader never ran. The e2e test caught it; the page now also listens for `hashchange`.
- **Port 3000 was taken** on my machine by an unrelated app, so the server ran with `BIND=127.0.0.1:3002` and the web app with `NEXT_PUBLIC_API_URL=http://127.0.0.1:3002`. Worth knowing when the API seems to answer with someone else's HTML.
- `vitest@5` wants `@types/node` 22 or newer; the scaffold had 20. Bumped to 22, which matches the Node version in the brief.
- Next 16 with `cacheComponents`: `useParams` in a client component on a dynamic route must sit inside `<Suspense>`, or the build fails. The escrow page wraps it.

### Known gaps found while building

- **A `CREATED` offer can't be withdrawn and never expires.** `cancel` refuses `CREATED` (there is no invoice to cancel yet) and the scheduler only looks at invoices and holds, so an offer the buyer never accepts stays in the operator queue forever. Small backend fix: let the seller or operator move `CREATED` straight to a final state.
- No "who am I" endpoint, hence the role detection above. `GET /api/v1/me` returning `{id, role}` would simplify sign-in.
- The release link uses `window.location.origin` unless `NEXT_PUBLIC_APP_URL` is set. For a phone on the LAN to open it, set that to an address the phone can reach (and add it to `CORS_ORIGINS`).

---

## 13. Phase 5, step 1: offers that can end, and `GET /api/v1/me`

Two gaps from section 12.

- **`CREATED` offers can now end.** Added the transition `CREATED -> EXPIRED_UNPAID` (no new state, so the web schema is unchanged). `cancel` on a `CREATED` escrow moves it there without any node call, since no invoice exists. The seller and operator can do it, and so can the buyer (declining). `tick` also expires a `CREATED` escrow once `now >= deadline_at`, so abandoned offers leave the operator queue by themselves. If the buyer accepts at the same moment, the compare-and-set decides and the loser gets `invalid_state`. The UI text for `EXPIRED_UNPAID` was reworded from "invoice expired" to "ended without a payment".
- **`GET /api/v1/me`** returns `{id, role}`. Sign-in now uses it, which removed the operator-endpoint probe, the 403 check and the optional user-id field.

Tests: 4 new API tests (withdraw, decline plus stranger gets 404, expiry at the deadline, `/me`) and one web test for `api.me`. `cargo test --workspace` and the 45 web unit tests pass; `npm run lint` and `npm run build` are clean. The Playwright test was not re-run in this step.

Environment note: this Codespace had no Rust or `protoc`; installed with rustup and `apt install protobuf-compiler`.

---

## 14. Phase 5, step 2: restricted macaroon and a restart test on the real node

- **Restricted macaroon.** `regtest/setup.sh` now bakes `toshi.macaroon` on the seller node with `invoices:read invoices:write info:read`, exactly what the adapter uses (add/settle/cancel/lookup/subscribe invoices, plus `GetInfo` for `/health`). Checked by hand: with it `getinfo` works and `listchannels` / `walletbalance` answer `permission denied`. `toshi-server` defaults to it, and the seller adapter in the real-node tests uses it, so those tests prove nothing more is needed. The buyer side of the tests still uses `admin.macaroon` because it plays a wallet.
- **Restart test** (`restart_mid_escrow_reconciles_with_the_live_node`, real LND, on-disk SQLite). Three escrows are open when the "process" dies: A and B locked, C invoiced but unpaid. While it is down the node cancels B's hold and the buyer pays C. A new store, adapter and service open the same file; `reconcile` leaves A `LOCKED`, moves B to `REFUNDED` and C to `LOCKED`, and then A and C settle with their secrets while B cannot be released. This is the "restart mid-escrow" acceptance criterion, now proven against a real node.
- **E2E.** Added two Playwright tests (withdraw and decline an offer; sign-in via `/me`). All four pass. The badge for `EXPIRED_UNPAID` now reads "Ended unpaid".
- `scripts/demo.sh` passes both paths with the server on the restricted macaroon; neither server nor web log contains a 64-hex string.

### Things that went wrong on the way

- **Containers could not talk to each other in this Codespace**, so LND sat forever after "Database(s) now open" and `setup.sh` timed out on "buyer wallet". Cause: the legacy iptables tables have `FORWARD DROP` and `bridge-nf-call-iptables=1` sends bridged traffic through them. Fix used: `sudo sysctl -w net.bridge.bridge-nf-call-iptables=0` (lasts until reboot), then restart the LND containers and re-run `setup.sh`.
- Playwright had no system Chrome here: used `npx playwright install chromium`, `npx playwright install-deps chromium`, and `PW_CHANNEL=chromium npm run e2e`.
- `adapter_settle_cancel_and_idempotency` failed once on the first run right after `setup.sh` and passed on every later run (three full runs); put down to the nodes still settling, not reproduced.

---

## 15. Phase 5, step 3: migrations and open-offer links

The seller no longer needs the buyer's user id: leave it empty and the escrow page is a link anyone can open.

- **Migrations** (`escrow-sqlite`). The schema is now an ordered list of SQL files (`src/migrations/001_initial.sql`, `002_open_offers.sql`); SQLite's `user_version` records how many have run. Each runs in its own transaction with foreign keys off (SQLite needs that to rebuild a table others reference) and a `foreign_key_check` before it commits. A database newer than the build is refused. `001` is the old `schema.sql`, so databases created before migrations existed (version 0, tables present) are adopted and upgraded in place. A test builds exactly such a database with an escrow and an audit row in it, opens it, and checks the data survived, `buyer_id` may now be empty, and the version is 2. The live demo database from earlier in this session was upgraded the same way when the new server opened it. Rule from now on: never edit a shipped migration, add a new one.
- **Model.** `Escrow.buyer_id` is `Option<String>`. `accept` on an open offer sets the buyer to the accepting user in the same compare-and-set transition that issues the invoice, so two people racing cannot both win; the loser gets `invalid_state`. (As before, the loser may have created an unused hold invoice on the node first; it expires on its own.) A named buyer cannot be replaced by someone else accepting, and the seller cannot accept their own offer.
- **API.** `buyer_id` is optional on create (blank counts as omitted; an unknown id is still rejected). Anyone signed in who has the link can read an open offer that is still `CREATED` and sees `your_role: "buyer"`; they cannot decline it (403), it is not in their list, and once someone has accepted it everyone else gets 404 again.
- **Web.** The form's buyer field is optional with an explanation. A visitor with no account who follows an offer link can create one right on that page (the token is shown once, as on the home page) and goes straight back to the offer. The seller's page warns that the first person to accept becomes the buyer. Tests: 6 Rust tests (core flows, API, store contract, migration) and 2 Vitest tests; the Playwright suite now has five tests, including a new-visitor open-offer run, and all pass.

Trade-off worth knowing: an open offer's link is a bearer capability. Whoever opens it first and accepts is the buyer, so it should be sent only to the person you are dealing with. Naming the buyer is still available.

Not done from the optional list: WebLN one-click payment, an in-app QR scanner, and a Postgres store.

---

## 16. Phase 5, step 4: WebLN, a camera scanner and a PostgreSQL store

- **WebLN** (`web/lib/webln.ts`). If the browser has a `window.webln` wallet, the invoice box offers "Pay with my wallet". The invoice goes to the user's own wallet; the app does not wait for the payment to finish, because a wallet may keep a hold invoice's `sendPayment` pending until the escrow ends (it stops waiting after 3 s), and it never says the funds are locked: the page changes only when the API reports `LOCKED`. A refused connection or a failed payment is shown. QR and copy-paste remain.
- **Camera scan** (`web/lib/scan.ts`, `components/escrow/Scanner.tsx`). On the seller's release form, "Scan the buyer's QR" uses the browser's own `BarcodeDetector` and camera (button only appears where both exist; frames never leave the device, no library). A scan only fills the secret field; releasing still takes a button press. Accepted: the buyer's release link for this escrow, or a bare 64-hex secret. Refused: a link for another escrow, short codes, 63/65 characters, non-hex, invoices, empty text.
- **PostgreSQL** (`escrow-postgres`). Same `Store` and `UserStore` traits, `sqlx` Postgres, a `schema_migrations` table and an advisory lock so two servers starting together cannot both migrate. State changes are the same compare-and-set inside one transaction (under READ COMMITTED the second writer re-checks the state and updates nothing). `toshi-server` uses it when `DATABASE_URL` is set, otherwise SQLite. The database comes from the compose file's `postgres` service behind a `postgres` profile, so `setup.sh` does not start it. It starts at the current schema (open offers included): there are no old Postgres databases to upgrade.

Verified: the shared store contract and user contract pass on a real PostgreSQL, plus a test that two concurrent transitions of one escrow yield exactly one winner and one audit entry, a duplicate payment hash is refused, and reconnecting keeps data. With `toshi-server` on PostgreSQL, `scripts/demo.sh` passes both demo paths against the real Lightning nodes; the rows end up `SETTLED` and `REFUNDED`, and no column in the schema could hold a secret. The Playwright suite has a sixth test: a fake WebLN wallet (bridged to `lncli`) pays, a stubbed camera and detector scan a wrong-escrow code (refused) and then the right one (fills the field, no release until pressed), then the release settles.

Not verified here: a real WebLN wallet extension and a real phone camera, since neither exists in this environment; the browser APIs were stubbed.

---

## 17. Sellers bring their own node

Lifts the "custody" non-goal's loose end: until now every seller shared one LND node, so after a release the sats sat on the operator's node (custodial). Now a seller can connect their own, and a release pays them directly.

**Design.** The escrow service used to hold one global node adapter. It now holds a `NodeDirectory` that returns the right node per seller (`escrow-core/src/adapter.rs`). Every escrow records `node: own | shared`, fixed at creation, so changing node settings later cannot strand an invoice. `LndDirectory` (`escrow-lnd/src/directory.rs`) builds a client per seller on first use, caches it, and replaces it when the seller changes their node. The event loop now finds the node for each open invoice instead of watching one node; a seller whose node is down is retried every 10 s and does not stop other sellers' escrows being watched or cancelled (tested). With `SHARED_NODE=off` the service has no node of its own at all.

**Security decisions.**
- *Credentials at rest:* the invoice macaroon is sealed with AES-256-GCM under `NODE_CREDENTIAL_KEY` (random nonce per seal; tamper, truncation and wrong key all refused). Without the key, connecting nodes is disabled instead of falling back to plaintext. API responses never contain the macaroon or certificate. The form clears the macaroon after saving.
- *Connection pinning:* the client library only reads credentials from files, so they are written to a 0700 temp directory with 0600 files, read by the connection, and deleted straight away. The connection is pinned to exactly the certificate the seller supplied.
- *SSRF:* the node address makes our server open a connection to a user-chosen host, so it must be `https://host:port` and resolve only to public addresses (loopback, private, link-local including the cloud metadata address, CGNAT, multicast, documentation and reserved ranges all refused, IPv4-mapped IPv6 included). A DNS answer can change between the check and the connect, so pinning is the second line. `ALLOW_PRIVATE_NODE_ADDRESSES=1` exists for regtest only and logs a warning.
- *Prove it works first:* saving connects, requires the node to be synced, creates a 1-sat hold invoice and cancels it; nothing is stored if any step fails. A macaroon from another node, or a certificate that is not the node's, is refused (tested on real LND).
- *No swapping mid-flight:* a node cannot be changed or removed while an escrow with an invoice is in flight on it (409 `node_in_use`). Invoice creation and node changes are serialised with a read/write lock so a swap cannot slip between the check and an `accept`.

**Persistence.** `escrows.node` (existing rows default to `shared`, which is what they were) and a `node_connections` table: SQLite migration 003, PostgreSQL migration 002. Both pass the extended store contract.

**API and UI.** `GET/PUT/DELETE /api/v1/me/node`; an escrow now carries `seller_node`; a seller with no node and no shared node gets 409 `no_node`. The web app has a "My node" page with the exact commands to bake and hex-encode an invoice-only macaroon, warns sellers on the shared node that the service holds their money, blocks escrow creation when no node is available, and tells buyers on each escrow page whether the funds go to the seller's own node or the service's.

**Verified.**
- Mock-node flow tests (3) and HTTP tests (8): invoices land on their own seller's node and no other, a downed node does not affect other sellers, and the rules above.
- Real LND (`each_seller_is_paid_on_their_own_node`): two sellers connect two different nodes with no shared node configured; each escrow's invoice exists only on its seller's node; both settle; each node's channel balance rises by exactly 1,000 sats; the stored macaroon is not in the database in the clear.
- Browser (new Playwright test): a seller is warned about the shared node, is refused with the wrong macaroon, connects `seller2`, runs a full escrow, and `seller2`'s balance rises by 1,000. The suite is 7 tests, all passing. Neither log contains a 64-hex string or the macaroon, and the database holds only ciphertext.
- Strict mode checked live: `SHARED_NODE=off` gives 409 `no_node`; loopback, `localhost`, `10.x`, `169.254.169.254` and `http://` addresses are all refused.

**Not done / limits.** The macaroon check cannot prove a macaroon has *only* the three invoice permissions (a broader one still works), so the UI tells sellers to bake a limited one. Sellers must run a reachable node with hold-invoice support. Hosted accounts with a withdrawal flow (Mode B) are still not built, so a seller on the shared node still has no way to be paid out. A rotated credential key needs a re-encryption step that does not exist yet.

**Environment note.** The Codespace restarted mid-session: containers came back with a corrupt layer and bitcoind had lost recent blocks, so LND's wallet was ahead of the chain ("Block height out of range"). Fix was `docker compose down -v` and `./regtest/setup.sh` (regtest is disposable). The `bridge-nf-call-iptables` setting also reset to 1 and had to be set to 0 again.

---

## 18. Arbitration

An optional trusted third party who decides disputes, so the buyer no longer holds all the power (before this, a buyer could keep the goods and simply never reveal `P`).

**The problem.** Refunding is easy: cancel the hold. Releasing needs `P`, which only the buyer's browser has. So the arbiter has to be able to obtain `P` without the server ever seeing it.

**The design.** When the buyer accepts an arbitrated escrow, their browser encrypts `P` to the arbiter's public key (ECIES from WebCrypto: ephemeral ECDH P-256, HKDF-SHA-256, AES-256-GCM with the recipient's key as additional data) and sends the 125-byte blob next to `H`. The server stores it, never returns it in any escrow view or in the history, and only serves it to the arbiter. The arbiter's browser opens it with a private key that never leaves their device.

Alternatives considered: a second hash path (Lightning has one hash lock, so no), the server holding `P` (custodial, and it could settle by itself), or Shamir shares (more moving parts for the same trust).

**Decisions worth recording.**
- *Junk blob problem.* The server cannot tell a valid blob from garbage, and a buyer could upload junk to defeat the seller's protection. So the arbiter must open the blob, check `sha256` against the payment hash and **confirm** before the seller relies on it; the seller's page warns until then. The confirmation is the arbiter's word, which the server records.
- *Key swap.* The arbiter's key is copied onto the escrow when it is created and shown with a fingerprint to buyer, seller and arbiter, so a later key change cannot redirect the buyer's encryption, and a swapped key would make the arbiter's confirmation fail.
- *Authority.* Only the named arbiter can decide, only while `DISPUTED`, only before the deadline. They cannot release, cancel, dispute or accept through the ordinary endpoints. The operator can no longer cancel a disputed arbitrated escrow, so nobody can overrule the arbiter; escrows without an arbiter are unchanged.
- *The record.* My first end-to-end run showed the history crediting the settlement to "the node": the node's own invoice event wins the race to record the state change, so the arbiter's name and note were lost. The decision is now written to the audit log before the node is touched. A second bug the tests then caught: a rejected attempt (wrong secret) was also being recorded as a decision, so the secret is validated before anything is written.

**Persistence.** Escrows gain `arbiter_id`, `arbiter_key`, `arbiter_sealed`, `arbiter_confirmed_at`; a new `arbiter_keys` table. SQLite migration 004, PostgreSQL migration 003. The "no place to keep a preimage" test was rewritten to inspect real column names instead of searching migration text for the word "secret" (a comment tripped it); the only column that mentions a secret is the encrypted `arbiter_sealed`.

**Verified.**
- Rust: 6 new flow tests and 10 HTTP tests (key registration and its validation, who can be named, key fixed at creation, sealed secret required and never leaked, access control for each endpoint, confirmation, release, refund, wrong or short secrets, deadline, the arbiter's lack of other powers, the operator unable to overrule). 106 workspace tests pass; SQLite and PostgreSQL pass the extended store contract; the real-node and Postgres suites still pass.
- Web: 8 crypto tests (round trip, wrong key, tampering in each part, wrong sizes, fingerprint format, SPKI shape) and schema tests; 84 unit tests in all.
- Browser, with real Lightning: a real arbiter creates a key, the buyer's secret is sealed, the arbiter verifies and confirms, the seller disputes, the arbiter releases and the seller is settled; separately the buyer disputes and the arbiter refunds. The test asserts the buyer's browser never sent `P`, the seller's never had it, and the arbiter's sent it exactly once, in the decision. The server accepted the keys the browser made, so the two implementations agree on the key format. A wrong device (no private key) and a stranger are turned away.
- The live database upgraded from schema 3 to 4 on startup with its data. No 64-hex string is in either log, and no stored blob contains a plaintext secret.

**Test infrastructure note.** The Playwright suite opened a new browser profile per party and never closed any; around the tenth test a tab crashed ("Target crashed") in a form fill. Not an app bug. Each test now closes its profiles when it ends. All 10 pass.

**Not done.** Arbiters are not paid, rated or discoverable; sellers must already know the arbiter's user id. No multi-arbiter or timeout-to-arbiter escalation. A lost arbiter key cannot be recovered (the blob can then only be refunded at the deadline). Hosted mode (Mode B), milestones and fiat display remain.

---

## 19. Demo preparation

The features were well ahead of the readiness work, so this step is about being able to show the product reliably.

**Decision: the lead use case is a courier delivery.** The recipient has to be physically present at the handover, which is exactly the moment the secret is shown; that answers "the buyer could just never release", and the arbiter scene covers what remains.

**What was built.**
- `scripts/demo-env.sh up|refresh|status|down|reset`: one command starts the Lightning network, server, demo wallet and web app, fixes the two environment problems we kept hitting (a stale chain making LND report "not synced", and the container-bridge firewall setting), and seeds the cast. From a clean state it takes about 25 seconds and is safe to repeat. State lives in `.demo/` (gitignored).
- `scripts/seed-demo.mjs`: a courier (seller) already connected to their own node `seller2`, a customer (buyer), and an arbiter whose key is generated, registered and saved as a backup the Arbiter page can restore. Reuses accounts while their tokens still work.
- `scripts/demo-wallet.mjs` and `web/components/DemoWallet.tsx`: a stand-in WebLN wallet so the presenter clicks "Pay with my wallet" instead of typing `lncli`. It listens on localhost only, accepts only `lnbcrt` invoices, never builds a shell command from input, and labels itself "Demo wallet · regtest" on screen. The web side does nothing unless `NEXT_PUBLIC_DEMO_WALLET_URL` is set.
- `docs/demo.md`: pitch, setup checklist, three scenes with talk track, honest limits, likely questions, and a troubleshooting table. Plus a top-level `README.md`, which the repo did not have.
- `web/e2e-demo/rehearsal.spec.ts` (`npm run rehearse`): plays the script exactly with the seeded cast, signing in by token, paying with the demo wallet, handing over by release link, and waiting out the 62-second refund. Takes about two minutes and screenshots each step at desktop and phone width.

**What the rehearsal found (and fixed).**
- The Next.js **"N" dev indicator** showed in every screenshot: turned off (`devIndicators: false`; errors still surface).
- On the arbiter's decision, **"Refund the buyer" was bright red** and "Release" plain, nudging the choice: both are now equal weight.
- The arbiter had to scroll to the history to find the dispute reason: it is now quoted at the decision ("What was reported").
- The seller saw a green **"you can start the work"** above an amber **"the arbiter hasn't confirmed, consider waiting"**: the green message now just says the funds are locked until the arbiter confirms.
- The demo wallet's on-screen label **never appeared**. React runs effects twice in development: the first run installed the wallet, the second saw it already there and returned before switching the label on. An assertion added to the rehearsal caught it.
- **A flaky browser suite had a real cause.** Failures landed on account creation: the server allows 10 sign-ups a minute per address, and the suite (and, in a real room, an audience behind one network address) exceeds that. The limits are now configurable (`RATE_LIMIT_PER_MIN`, `REGISTER_LIMIT_PER_MIN`), the demo environment raises the sign-up limit, and the one other failure (a note appearing twice in the history, so a selector was ambiguous) was fixed in the test.
- `demo-env.sh` itself: a first version hung when its output was piped, because `a && b &` backgrounds a whole subshell that keeps the pipe open. Each service now gets its own redirected, `exec`'d process. `down` also stops services whose pid file was lost.

**Verified.** From `reset` then `up` the rehearsal passes with a brand-new cast, and the 10-test browser suite passed twice in a row against the same environment; 84 web unit tests, lint and build are clean.

**Not done.** Still regtest with a stand-in wallet: nothing here proves real phone wallets hold a payment pending for hours. Releasing from a real phone over a LAN needs HTTPS for the in-app camera (the phone's own camera app opening the link works without it).

---

## 20. Fiat display

The PRD's FR-16: an optional fiat value shown beside the sats, quote locked at creation, display only, no conversion.

**Decisions.**
- *The sats are the truth.* The escrow stays in sats; nothing in the escrow logic reads the fiat value (it is not even in the state machine). The form does not accept a fiat amount to convert into sats, which would make it a conversion.
- *Locked at creation.* The seller picks a currency; the server takes a rate once and stores the currency, rate, computed amount, time and source. Rate and amount are integers in the currency's minor unit (minor units per bitcoin, rounded half up in 128-bit), so no float is ever stored; the web preview does the same arithmetic with BigInt, and the tests pin the two to the same answers.
- *The server fetches rates, the browser never does*, preserving the "no third-party calls from the page" rule. `FIAT_PROVIDER` selects `off` (default), `coingecko`, `mempool` or `fixed:...` (labelled "demo rate", because regtest has no market). Live rates are cached for a minute and a stale one served for up to ten minutes if the feed is down.
- *Failure is explicit.* If a value was requested and cannot be produced, creation fails (`rate_unavailable`, `unsupported_currency`, `fiat_disabled`) instead of silently dropping it. Leaving the value out always works, so a price feed can never block an escrow. A zero, negative, non-finite or absurd rate is treated as a broken feed.
- *Honest framing in the UI.* "≈ ₦4,875.00 when offered", the rate, source, time and "display only", with a tooltip saying bitcoin's price moves.

**Persistence.** Five nullable columns on `escrows` (currency, amount, rate, quoted-at, source): SQLite migration 005, PostgreSQL migration 004. The shared store contract now round-trips a quote and checks a transition leaves it alone.

**Verified.**
- Rust (131 workspace tests, up from 106): 7 on the quote maths (rounding, yen with no decimals, overflow refused, unsupported currencies, bad rates), 11 on providers (both feeds' JSON shapes, garbage and broken prices refused, the cache making one call per minute, stale-while-error for ten minutes then giving up, recovery, junk never poisoning the cache) and 7 over HTTP (off by default, the rates endpoints, the same locked value for buyer, seller and list, the value staying put when the market halves and then the feed dies, through settlement, explicit errors with nothing created, the sats limits unchanged, idempotency keys covering the currency).
- Against the real services: both live feeds return the shapes the parsers expect (CoinGecko all 13 currencies, mempool.space its 7 without naira), and an opt-in test (`cargo test -p escrow-api --lib -- --ignored live_feeds`) pulls sane prices through the real HTTP client.
- Web: 12 new unit tests (the integer maths with the server's own examples, formatting for dollars, naira and yen, schemas, the form). Browser: a new test chooses naira, sees the preview, creates the escrow, and checks the buyer's page and list show the same locked value; the rehearsal's first scene now shows it. All 11 browser tests and the 3-scene rehearsal pass.

**A regression it exposed.** The form now has two `<select>` elements (currency and hold unit), and every test selected the hold unit with a bare `locator("select")`. The hold-time amount and unit now have their own accessible names (`Hold time amount`, `Hold time unit`), which is better for screen readers too, and the tests use them. Exact-match on "Hold time" hung because the wrapping label also contains the hint text.

**Not done.** No fiat amount entry (that would be conversion), no live "worth now" figure on an existing escrow, no per-user default currency, and no currencies beyond the 13. The demo environment uses fixed demo rates; a real deployment sets `FIAT_PROVIDER=coingecko` or `mempool`.

---

## 21. A signet attempt that was abandoned

Tried to test our hold timing on the real public signet with two LND nodes of our own, since regtest blocks are instant and real ones are not. Research first: almost no mobile wallets support signet Lightning, so it could answer the timing question but not the real-wallet one. Then the environment pushed back: the disk had 4 GB free (freed by deleting 4 GB of rebuildable compiler cache), and containers here have no internet access, so bitcoind had to run on the host network. A pruned signet node did sync (5% of headers in the first minute, 41% after about nine), but the work was abandoned as taking too much space, and every file, container and volume from it was removed; none of it was committed. The two questions it was meant to answer are still open: whether our timing numbers hold on real 10-minute blocks (the 71-block hold window and LND's 12-block margin were measured only on regtest), and how real phone wallets treat a payment that stays pending for hours.

---

## 22. A landing page, and opening the app from a Codespace

**Landing page** (`web/components/landing/`). Signed-out visitors to `/` now get a designed landing page instead of two bare forms; signed-in people still get their escrows. Inspired by the structure of a polished crypto-escrow site (a product card in the hero, a "which side are you on" split, numbered steps, trust badges, a stat row, one closing call to action, a rich footer), but in our own identity and our own words: a dark hero with an amber Lightning glow and a faint grid, then light/dark-aware sections. Sections: hero with a live escrow card that cycles awaiting payment, locked and settled; paying vs getting paid; four steps; the three ways a deal ends (paid on handover, refunded automatically, decided by an arbiter); why you can trust it; honest limits; and the real create-account and sign-in forms as the closing call to action, then the footer.

Decisions:
- *Honest copy.* The stat row holds only verifiable facts (a 256-bit secret made in your browser, automatic refund at the deadline, a 6 hour maximum hold, the released payment landing on your node), not invented user counts or volume. A "what it doesn't do" section says plainly that releasing proves the buyer chose to release and not that the job was good, that holds are short, and that it runs on a test network with stand-in coins and has not been tried with real wallets. The hero says "no real money in it yet".
- *No new dependencies, no third parties.* Inline SVG icons, system fonts, CSS-only animation. The cycling card respects `prefers-reduced-motion` (it then stays on "Funds locked"), as do the entrance animations and smooth scrolling.
- *The shell steps aside.* The app was a narrow centred column with a header and footer. A new `AppShell` gives a signed-out visitor on `/` a full-width page with its own navigation and footer, and everyone else the app layout.

**A bug the browser tests caught, and why it matters.** After the change, every one of the 11 browser tests failed at account creation: "Save your token now" never appeared. Creating an account signs you in immediately (so a closed tab can't lose it), which flipped the shell from the landing layout to the app layout. Those layouts wrapped the page in *different elements*, so React threw the home page away and rebuilt it, which erased its one-time "show the token" state. The token is shown once and cannot be recovered, so a visitor would have been signed in to an account whose token they had never seen. The fix is a wrapper that stays the same element in both modes, with only its classes and the optional header and footer around it changing; it is explained in a comment on `AppShell`. A dedicated browser test now covers the whole landing path (the pitch and every section, the calls to action, making an account from the landing and keeping the token screen, and signing out back to the landing).

Smaller things the screenshots and dev tooling found: the closing section's white text was inherited by its white form cards, making their titles invisible; the headline left a single word on its own line (now balanced); Next 16's dev tooling flagged smooth scrolling without `data-scroll-behavior="smooth"` on `<html>`, and flagged the home page as dropped from rendering while the app decides whether anyone is signed in (the page now renders inside a placeholder in the landing's colour).

**Opening the app from a Codespace.** The web page called the API at `127.0.0.1:3000`, which only works through VS Code's local port forwarding; in a browser tab it points at your own machine. Cross-origin calls to a second private forwarded port also fail. So the web app can now proxy the API and the demo wallet through its own port (`API_PROXY_TARGET`, with `NEXT_PUBLIC_API_URL=""`): the browser only ever talks to one origin, no other port has to be exposed, and there is no CORS. It is off unless that variable is set; `scripts/demo-env.sh up` sets it, tells the demo wallet to accept the forwarded address, and prints the browser URL.

**Environment note.** The Codespace restarted again and the Docker containers came back corrupted ("RWLayer ... is unexpectedly nil"), as before. Wiping and rebuilding the regtest network fixed it; that is the only reliable recovery.

**Verified.** Lint, build and 96 unit tests pass. All 12 browser tests and the 3-scene demo rehearsal pass. The page was checked in screenshots at desktop and phone width, in light and dark.

**Not done.** No real product screenshots or illustrations beyond the sample card; the landing is a single page with no separate pricing or docs pages; and the in-app pages (new escrow, the escrow page, node and arbiter setup) are untouched and could use the same design pass.

---

## Next

Try real wallets on a public network (signet). The demo is prepared and rehearsed. Optional extras are all done (WebLN, scanner, Postgres, open-offer links, migrations); a restricted-permission review of production deployment (TLS, reverse proxy and rate-limit addresses) is not. WebLN one-click payment, an in-app QR scanner.
