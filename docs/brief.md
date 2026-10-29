# Brief: Phase 4, the web frontend

You are taking over the frontend of **toshi-way**, a Bitcoin Lightning escrow. The backend is finished and verified against real Lightning nodes. What is missing is the web app: `web/` is still the stock `create-next-app` template. Your job is to build the screens so a buyer and a seller can run the full flow in a browser.

Read this file first, then skim `docs/project.md` (what the product is) and `docs/flow.md` (every decision and every problem we hit, in order).

## 1. The product in one minute

A buyer and a seller who don't trust each other agree a price. The buyer's Lightning payment is **locked** (a HODL invoice) rather than paid. Two outcomes:

- **Release:** the buyer (or whoever receives the goods) hands the seller a 32-byte secret `P`. The seller submits it, the payment settles, the seller is paid.
- **Refund:** if `P` is never given before the deadline, the payment is cancelled and the buyer gets the money back automatically.

Nobody holds the money; the lock is the escrow. The backend ties everything to the Lightning node's real state.

The most important rule for you: **the browser never decides that money is locked, settled or refunded.** It only displays the `state` the API reports.

## 2. Get it running

You need Docker (with Compose), Rust, Node 22, and `protoc` on your PATH (the LND client generates code from protobuf files; on Linux download a release from github.com/protocolbuffers/protobuf, or `apt install protobuf-compiler`).

```bash
# 1. Lightning regtest network (bitcoind + 3 LND nodes: buyer, router, seller). Idempotent, safe to re-run.
./regtest/setup.sh

# 2. The API server on http://127.0.0.1:3000
export OPERATOR_TOKEN=$(openssl rand -hex 16)      # 24+ characters; keep it, it is the operator's login
cargo run -p escrow-api --bin toshi-server

# 3. The frontend on http://localhost:3001 (the API owns 3000)
cd web && npm install && npm run dev -- -p 3001
```

The server allows browser requests from `http://localhost:3001` and `http://127.0.0.1:3001`. To allow other origins set `CORS_ORIGINS` (comma separated) before starting it.

To see the whole flow without any UI, run `OPERATOR_TOKEN=... ./scripts/demo.sh`. It performs both demo paths with `curl`; **it is the best executable reference for the API calls.**

### Paying an invoice in development

There is no wallet in the dev environment. Pay the buyer's invoice from the buyer node:

```bash
docker compose -f regtest/docker-compose.yml exec -T buyer \
  lncli --network=regtest payinvoice --force <paste the payment_request>
```

The command blocks while the funds are held, which is correct. The escrow becomes `LOCKED` within about a second.

### Things that will bite you

- **`/health` says `degraded` / node `down`.** Regtest only mines blocks on demand, and LND reports "not synced" when the newest block is over two hours old. This is correct behaviour, not a bug. Fix:
  ```bash
  docker compose -f regtest/docker-compose.yml exec -T bitcoind bitcoin-cli -regtest -rpcuser=toshi -rpcpassword=toshi loadwallet miner
  docker compose -f regtest/docker-compose.yml exec -T bitcoind bitcoin-cli -regtest -rpcuser=toshi -rpcpassword=toshi -rpcwallet=miner -generate=1
  ```
- **Payments fail with `NO_ROUTE` or `INSUFFICIENT_BALANCE`.** Peer links between the nodes drop when containers restart. Re-run `./regtest/setup.sh`.
- **Next.js 16 / React 19 are newer than most examples online.** `web/AGENTS.md` warns about this; the framework's own docs ship inside `web/node_modules/next/dist/docs/`. Check them before assuming an API works as you remember.
- The shortest legal hold is 60 seconds. For testing the refund path use `hold_secs: 62`.

## 3. The API contract

Base URL `http://127.0.0.1:3000` (put it in `NEXT_PUBLIC_API_URL`). JSON everywhere. Times are **unix seconds**. Amounts are **sats**.

Authentication is `Authorization: Bearer <token>`. `GET /health` and `POST /api/v1/users` need no token.

| Method | Path | Who | Body | Returns |
|---|---|---|---|---|
| POST | `/api/v1/users` | anyone | none | 201 `{id, role, token}`. **The token is shown once and cannot be recovered.** |
| POST | `/api/v1/escrows` | seller | `{buyer_id, amount_sats, description, hold_secs}` | 201 escrow. Send an `Idempotency-Key` header to make retries safe. |
| GET | `/api/v1/escrows` | any user | | `{escrows: [...]}` the caller's own, newest first |
| GET | `/api/v1/escrows/{id}` | buyer, seller, operator | | escrow |
| POST | `/api/v1/escrows/{id}/accept` | buyer | `{payment_hash}` (64 hex) | escrow, now `INVOICE_ISSUED` |
| GET | `/api/v1/escrows/{id}/invoice` | buyer | | `{payment_request, expires_at}` (only while payable, else 409) |
| POST | `/api/v1/escrows/{id}/release` | buyer, seller | `{preimage}` (64 hex) | escrow |
| POST | `/api/v1/escrows/{id}/cancel` | see below | none | escrow |
| POST | `/api/v1/escrows/{id}/dispute` | buyer, seller | `{reason}` (1 to 500 chars) | escrow |
| GET | `/api/v1/escrows/{id}/events` | buyer, seller, operator | | `{audit: [...], node_events: [...]}` |
| GET | `/api/v1/operator/escrows` | operator | | `{escrows: [...]}` all active ones |
| GET | `/health` | anyone | | `{status, node, server_time}`; 503 when the node is unreachable |

An escrow looks like:

```json
{
  "id": "f7e255b8-...", "buyer_id": "u_5f26...", "seller_id": "u_cf4e...",
  "amount_sats": 1000, "description": "demo parcel",
  "payment_hash": "68af07...",            // null until the buyer accepts
  "state": "LOCKED",
  "invoice_expires_at": 1792666545,       // null until accepted
  "deadline_at": 1792667399, "created_at": 1792665599, "updated_at": 1792665600,
  "your_role": "buyer"                    // "buyer" | "seller" | "operator"
}
```

The invoice string is **never** part of the escrow object. It only comes from `/invoice`.

Audit entries: `{actor, action, from_state, to_state, reason, at}`. Node events: `{node_invoice_state, at}`.

Create limits: amount 1 to 1,000,000 sats; `hold_secs` between 60 and 21,600 (6 hours); description up to 500 characters; the buyer must be an existing user id.

### Who can do what

- Only the **buyer** can accept and fetch the invoice.
- **Buyer or seller** can release and open a dispute.
- **Cancel:** the seller and the operator can cancel at any time (the buyer gets their money back). The **buyer can only cancel before paying** (`INVOICE_ISSUED`); once funds are locked a buyer-only cancel would strip the seller's protection, so the API answers 403.
- The **operator** can read any escrow and the queue, and can cancel, but cannot release, dispute, or be a party.
- Someone with no standing on an escrow gets **404**, not 403, so ids can't be probed. Treat 404 as "doesn't exist or isn't yours".

### States

`CREATED` (offer made) → `INVOICE_ISSUED` (buyer accepted, waiting for payment) → `LOCKED` (funds held, the node confirmed) → `SETTLED` (seller paid) or `REFUNDED` (buyer refunded). Side exits: `EXPIRED_UNPAID` (the invoice expired unpaid), `DISPUTED` (flagged while locked), `FAILED` (needs operator attention). `SETTLED`, `REFUNDED`, `EXPIRED_UNPAID` and `FAILED` are final.

### Errors

Always `{"error": {"code": "...", "message": "..."}}`. Branch on `code`, show `message`.

| Code | HTTP | Meaning |
|---|---|---|
| `unauthorized` | 401 | missing or wrong token: send the user to sign in |
| `forbidden` | 403 | you are a party but not allowed this action |
| `not_found` | 404 | no such escrow, or it isn't yours |
| `invalid_state` | 409 | action not possible in the current state: refetch and re-render |
| `wrong_preimage` | 422 | the secret doesn't match this escrow |
| `invalid_preimage` / `invalid_hex` | 400 | not exactly 64 hex characters |
| `too_late` | 409 | the deadline has passed |
| `invalid_deadline` / `invalid_amount` / `invalid_description` / `invalid_reason` / `unknown_buyer` / `invalid_body` / `invalid_request` | 400 or 422 | form validation |
| `hash_in_use` / `conflict` / `idempotency_key_reuse` | 409 / 422 | retry or regenerate |
| `rate_limited` | 429 | wait `Retry-After` seconds |
| `node_unavailable` | 503 | the Lightning node is down: tell the user to retry; nothing changed |
| `node_rejected` | 502 | the node refused |

## 4. What to build

Next.js App Router, TypeScript, Tailwind, **Zod** for form validation and for parsing API responses (the PRD asks for Zod).

### Screens

1. **Home / account.** If there is no token: "Create account" (`POST /users`) and "Sign in with a token". After creating an account, show the token **once** with a copy button and a clear warning that it can't be recovered; show the user's id too (a seller needs a buyer's id to invite them). If signed in: list my escrows (`GET /escrows`) with state, amount, role, and a link to each. A sign-out button.
2. **New escrow (seller).** Fields: buyer id, amount in sats, description, hold time (offer minutes/hours up to the 6 h maximum). Generate an `Idempotency-Key` per form submission. On success go to the escrow page and show a shareable link to it.
3. **Escrow page** `/escrows/[id]`, polling `GET /escrows/{id}` every 2 seconds, rendering by `state` and `your_role`:

   | State | Buyer sees | Seller sees |
   |---|---|---|
   | `CREATED` | "Accept": generates the secret, sends only the hash | "Waiting for the buyer to accept" |
   | `INVOICE_ISSUED` | The invoice as a **QR code and copyable text**, countdown to `invoice_expires_at`, "Cancel" | "Waiting for payment" |
   | `LOCKED` | "Funds are locked", countdown to `deadline_at`, **release QR** (hidden until revealed), "Open dispute" | "Funds are locked: you can start the work", countdown, a field to paste the secret (or auto-filled from a scanned link), "Release payment", "Give refund" (cancel), "Open dispute" |
   | `DISPUTED` | explain that the operator will review; the deadline still applies | same |
   | `SETTLED` | success summary | "You were paid" |
   | `REFUNDED` | "You were refunded" | "Cancelled, buyer refunded" |
   | `EXPIRED_UNPAID` | invoice expired | same |
   | `FAILED` | "Needs operator attention" | same |

   Also show the event timeline from `/events`, and a clear note for each state of **what happens next and what the deadline means**.
4. **Operator** `/operator`: sign in with the operator token; list the queue, highlight `DISPUTED`, with a Cancel button per escrow.

### The secret `P`: handle with care

This is the security-critical part. Follow it exactly.

- **Generate in the browser:** 32 random bytes from `crypto.getRandomValues`, hex-encoded (64 characters). Compute `H = sha256(P)` with `crypto.subtle.digest` and send **only `H`** to `/accept`. `P` never goes to the server except when the seller submits it to `/release`.
- **Keep it on the buyer's device** in `localStorage` under a key containing the escrow id. Tell the buyer plainly: if this browser's data is cleared, the secret is lost and the escrow can only end in a refund.
- **Hand-over to the seller by QR / link:** the QR encodes `https://<app>/escrows/<id>#release=<64 hex>`. The secret sits in the URL **fragment**, which browsers never send to servers, so it cannot reach access logs. The escrow page reads `location.hash`, immediately removes it with `history.replaceState`, and offers a "Release payment" button (do not release automatically on page load). A phone camera scanning the QR and opening the link is enough; an in-app scanner is optional.
- Copy/paste of the 64-hex secret must also work as a fallback. **Never accept short codes.** A 6-digit code could be brute-forced by the seller from the hash. Validate "exactly 64 hex characters" client-side too (the server enforces it regardless).
- Never log `P`, never put it in a query string, never send it to analytics, never render it in an element that third-party scripts could read. No third-party scripts at all.
- Generate QR codes **locally** (for example the `qrcode.react` package). Never send an invoice or secret to an online QR service.

### Other rules

- **The UI reflects state, it never infers it.** After the buyer pays, do not show "locked" because the user clicked "I paid". Wait for the API to say `LOCKED`.
- On `invalid_state`, refetch and re-render instead of showing a scary error. On `node_unavailable`, say the node is temporarily unavailable and that nothing changed.
- Keep the token in `localStorage` (acceptable for this project), read it in one small module, attach it in one API client, and handle 401 centrally by signing out.
- Responsive down to phone width: the QR hand-over is meant to happen on a phone.
- Show amounts in sats. Show times as a countdown and as a local time.

### Suggested structure

```
web/lib/api.ts        typed client: base URL, bearer token, error parsing, 401 handling
web/lib/schemas.ts    Zod schemas for escrow, events, errors, forms
web/lib/secret.ts     generate P, hash H, parse/validate hex, build and parse release links
web/lib/session.ts    token + user id in localStorage
web/app/...           pages above
```

## 5. One backend gap you should know about

**Clock skew.** The countdowns compare `deadline_at` (server clock) with the browser's clock. If the user's clock is wrong the countdown lies, and a user may think they have time when the server will refuse with `too_late`. The API has no server-time field yet. The cleanest fix is to add `server_time` (unix seconds) to the `/health` response or to the escrow view, and have the client compute an offset. It is a few lines in `escrow-api/src/routes.rs`. Make this small change yourself or ask the repo owner. Until then, treat the countdown as indicative and let the server's answer win.

*Done in phase 4 step 2: `/health` returns `server_time` and the web app corrects its countdowns with it (see `docs/flow.md`, section 12).*

## 6. Testing and done criteria

- **Unit tests (Vitest or similar):** secret generation is 32 bytes and 64 hex characters; `sha256` matches a known vector (`sha256` of 32 zero bytes is `66687aadf862bd776c8fc18b8e9f8e20089714856ee233b3902a591d0d5f2925`); short, non-hex and 63/65-character inputs are rejected; release links round-trip; Zod schemas accept the sample escrow above and reject malformed input.
- **Keep `npm run lint` and `npm run build` clean.**
- **End-to-end by hand (required before you call it done):** with the regtest network and server running, use two browser profiles (or one normal and one private window) as seller and buyer, and complete both demo paths through the UI:
  1. **Happy path:** seller creates, buyer accepts and pays the invoice, both see `LOCKED`, buyer shows the release QR, seller opens the link and releases, both see `SETTLED`.
  2. **Refund path:** a 62-second escrow is paid and locked, nobody acts, both screens flip to `REFUNDED` by themselves.
  Also try: wrong secret (clear error, state unchanged), the buyer trying to cancel while locked (blocked), a stranger opening the link (404 handled), and stopping the server mid-escrow (UI recovers when it returns).
- An automated browser test (Playwright) against the live stack is a good stretch goal; model it on `scripts/demo.sh`.

**Done means:** both demo paths run end to end in the browser, the secret never appears in any server log or network request other than the release call, and lint, build and tests pass.

## 7. Known limitations (not your job, but don't be surprised)

- ~~The seller invites a buyer by user id.~~ Solved in phase 5 (see `docs/flow.md`, section 15): `buyer_id` is optional on create, and the first user to accept an open offer becomes its buyer.
- The idempotency cache is in server memory; it resets when the server restarts.
- Arbitration exists (see `docs/flow.md`, section 18): an escrow can name an arbiter, and the buyer's browser seals the release secret to them.
- The server's shared node uses an invoice-only macaroon (`toshi.macaroon`); sellers can also connect their own nodes (see `docs/flow.md`, section 17).
- Hold times are limited to 6 hours by what the Lightning network safely supports (see `docs/flow.md`, section 7). Don't offer longer.
- Wallet integration (for example WebLN, so the buyer can pay in one click) is not built. Showing the invoice as a QR and text is the requirement.

## 8. Working agreements

- Work on a branch (for example `frontend`) and open a pull request into `main`. Keep commits small and in your own name.
- You should not need to change Rust. If you do (for example the `server_time` field), keep the change small and run `cargo test --workspace` before pushing; all tests must stay green.
- Update `docs/flow.md` with a short entry per meaningful step (what you did, what surprised you), the same way the earlier sections are written, and tick items off in the status list in `docs/project.md`.
- Don't commit `node_modules`, `.next`, `.env*`, `regtest/creds/` or `*.db` (all are gitignored).

## 9. Where things are

| Path | What |
|---|---|
| `docs/project.md` | the product spec and status |
| `docs/flow.md` | chronological log of decisions, measurements and problems |
| `escrow-core/` | state machine, timing rules, service logic (no I/O) |
| `escrow-sqlite/` | SQLite store |
| `escrow-lnd/` | LND adapter, event loop, deadline scheduler |
| `escrow-api/` | the HTTP API and the `toshi-server` binary |
| `escrow-api/tests/api.rs` | request and response examples for every endpoint |
| `scripts/demo.sh` | both demo paths over HTTP with `curl` |
| `regtest/` | Docker Compose Lightning network and its setup script |
| `web/` | the frontend you are building |
