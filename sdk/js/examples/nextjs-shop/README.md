# Example shop (Next.js)

A small shop that takes payment through toshi-way. It shows the whole business side in about 150 lines:

| File | Does |
|---|---|
| `app/page.tsx` | A product and a **Pay with toshi-way** button (a plain form post) |
| `app/api/checkout/route.ts` | Makes the order, makes its escrow, and redirects the customer to toshi-way to pay |
| `app/api/toshi-webhook/route.ts` | Receives toshi-way's webhooks: verifies, handles each event once, updates the order |
| `app/orders/[id]/page.tsx` | Where the customer lands afterwards. It shows the order's status and refreshes itself |
| `lib/orders.ts` | A tiny JSON-file order store. Use your database in production |
| `lib/toshi.ts` | The API client, created once on the server |

## Run it

You need a toshi-way server (the regtest setup is fine) and an account on it.

1. In the toshi-way web app, open **API**. Make a key with both scopes, then add a webhook with the address
   `http://127.0.0.1:3200/api/toshi-webhook`. Copy the key and the signing secret: each is shown once.
2. Set up this app:

   ```bash
   cd sdk/js/examples/nextjs-shop
   cp .env.example .env.local      # then fill in TOSHI_KEY and TOSHI_WHSEC
   npm install
   npm run dev                     # http://localhost:3200
   ```

3. Open the shop and press **Pay with toshi-way**. You land on toshi-way's escrow page. Make a customer
   account, accept, and pay the invoice. The shop's order page moves from *Waiting for payment* to *Paid and
   locked* by itself, because toshi-way's webhook told the shop.
4. As the seller (the account that owns the key), release with the customer's secret. The order becomes
   *Complete*.

## What to copy into your own shop

- **Create the escrow on your server**, with your order number as `reference` and as the `idempotencyKey`, so
  a retried request can never make two escrows. The key stays on the server.
- **Redirect to `checkout_url`.** The customer pays on a toshi-way page, never inside your site. If your
  server has no public web address the API returns no `checkout_url`; then build it from the web address
  (`TOSHI_WEB_URL` here).
- **Never treat the redirect back as payment.** The customer may never return. The order only changes when a
  signed webhook says so.
- **Read the raw body in the webhook** (`request.text()`), never `request.json()`: the signature covers the
  exact bytes sent.
- **Handle each event once** (the event `id`), and answer a failure with 500 so toshi-way retries.
- **Before marking an order paid, read the escrow from the API** and check it is `SETTLED`. Only `SETTLED`
  means you have been paid: `LOCKED` money can still be refunded.

The example's order store is a file and is not safe across several servers. Put orders and handled event ids
in a database, and make "handle this event" a single transaction.
