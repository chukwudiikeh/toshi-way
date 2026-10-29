# toshi-way web

The browser app for the toshi-way Lightning escrow: Next.js (App Router), TypeScript, Tailwind and Zod. It talks to `toshi-server` over HTTP and only displays the state the API reports.

## Run it

Start the regtest network and the API first (see `docs/brief.md`, section 2), then:

```bash
npm install
npm run dev -- -p 3001          # the API owns port 3000
```

| Variable | Default | Purpose |
|---|---|---|
| `NEXT_PUBLIC_API_URL` | `http://127.0.0.1:3000` | Where `toshi-server` listens |
| `NEXT_PUBLIC_APP_URL` | the page's own origin | Base of the links put in QR codes. Set it to an address a phone can reach (and add that origin to the server's `CORS_ORIGINS`) to hand over on a real phone |

## Screens

- `/`: create an account (the token is shown once) or sign in with a token; your escrows.
- `/escrows/new`: seller creates an offer for a buyer's user id.
- `/escrows/[id]`: the escrow by role and state: accept, pay the invoice (QR), release QR hand-over, release, refund, dispute, history. Polls every 2 seconds.
- `/operator`: the operator queue, disputes first, with cancel.

## Code map

| Path | What |
|---|---|
| `lib/api.ts` | The only API client: base URL, bearer token, Zod-parsed responses, error format, 401 sign-out |
| `lib/session.ts` | The only code that touches the token in `localStorage` |
| `lib/secret.ts` | The release secret: generate, hash, validate, release links, per-escrow storage |
| `lib/schemas.ts` | Zod schemas for API responses and forms |
| `lib/clock.ts` | Server-corrected time for countdowns (`/health` → `server_time`) |
| `components/escrow/` | The escrow page: polling hooks, per-state panels, history |

## The release secret

The buyer's browser generates `P` (32 random bytes), stores it under `toshi.secret.<escrow id>` and sends only `sha256(P)`. The seller receives `P` from a QR code of `/escrows/<id>#release=<P>`. The fragment never reaches a server, and the page removes it from the address bar as soon as it reads it. `P` is sent only in the release call. Only full 64-hex secrets are accepted. Don't add third-party scripts: anything on the page can read `localStorage`.

## Checks

```bash
npm run lint
npm run build
npm test        # Vitest unit tests (tests/)
npm run e2e     # Playwright against the live stack, using the system Chrome (e2e/)
```

`npm run e2e` needs the regtest network, `toshi-server` and `npm run dev -- -p 3001` running. It pays invoices from the buyer node with `lncli` through `docker compose`. Set `WEB_URL` if the app is not on `http://localhost:3001`, and `PW_CHANNEL` to use another installed browser.
