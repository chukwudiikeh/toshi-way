# Demo script: escrow for deliveries, on Lightning

**Use case: a courier delivery.** A courier (the seller) takes a job; the customer (the buyer) pays up front, but the money is *locked*, not sent. It is released to the courier only when the customer, holding the parcel, shows a QR code. If that never happens, the customer is refunded automatically. If they disagree, a trusted arbiter decides.

Why delivery: the recipient has to be physically present at the handover, which is exactly the moment the secret is shown. That answers the obvious objection ("the buyer could just never release"), and the dispute scene covers what is left of it.

**Three scenes, about 10 minutes plus questions:**

| Scene | Story | Takes |
|---|---|---|
| 1. Paid on handover | Customer pays, gets the parcel, shows the QR, courier is paid | ~3 min |
| 2. Nobody shows up | Courier never arrives; the customer is refunded with no one lifting a finger | ~2 min (a 62 s wait) |
| 3. A dispute | Customer won't release; the arbiter reads the evidence and pays the courier | ~4 min |

## Before you start (about 5 minutes)

```bash
./scripts/demo-env.sh up        # Lightning network, server, demo wallet, web app, and the cast
./scripts/demo-env.sh status    # everything should say "up"
./scripts/demo-env.sh refresh   # right before going on: keeps the Lightning node reporting "synced"
```

`up` takes about 25 seconds and is safe to run again. It prints the app URL, the operator token, and creates three accounts, saved with their tokens in `.demo/accounts.json`:

| Who | Role | Notes |
|---|---|---|
| **courier** | seller | already connected to their **own** Lightning node, so their money lands with them, not with the service |
| **customer** | buyer | |
| **arbiter** | referee | has a key ready; its backup is `.demo/arbiter-key.json` |

**Set up three browser windows** (use separate profiles or private windows so they don't share sign-in), arranged side by side: **courier** (left), **customer** (right; narrow it to phone width if you can, since this is the screen that shows the QR), **arbiter** (a third window, hidden until scene 3). Sign each in by pasting the token from `.demo/accounts.json` on the home page.

For the arbiter, open **Arbiter**, paste the contents of `.demo/arbiter-key.json` into *Restore from a backup*, and press **Restore**. (This is the "new device" step; the key never lived on the server.)

A small "Demo wallet · regtest" label sits in the corner. It is a stand-in for a phone wallet: say so out loud. Its button pays from the customer's regtest node.

**Run the rehearsal once first** if you have the time: `cd web && npm run rehearse`. It plays all three scenes below with the same accounts and takes about two minutes, and leaves screenshots in `web/rehearsal-screenshots/`.

## The 30-second pitch

> "Pay-first means the seller can run off with the money. Deliver-first means the buyer can take the goods and not pay. Escrow services fix that by holding your money, which means trusting them. Lightning has a feature called a hold invoice: the payment is *locked on the network* and the seller's node can only claim it with a secret that the buyer holds. Nobody holds the money. We built the product around that."

## Scene 1: paid on handover (~3 min)

1. **Courier window.** *New escrow.* Leave the buyer empty. Description `Parcel #4821 to Lekki, Lagos`, amount `5000`, hold `30` minutes. **Create escrow.**
   - Point out: **Funds go to: The seller's own node.** "The courier connected their own node, so when this settles the sats land with them."
   - "I left the buyer empty, so this page is a link I can send to the customer."
2. **Customer window.** Open that link (paste the URL). **Accept offer.** "Their browser just made a secret and sent only its *hash* to the server. The secret never leaves this device."
3. Press **Pay 5,000 sats with my wallet.** Both windows flip to **Funds locked** within a second or two.
   - Point out: the page changed because the **Lightning node reported it**, not because anyone clicked "I paid". "The website never decides that money is locked."
   - Courier: "I can start the work: the payment is committed but I can't touch it."
4. **Handover.** Customer: **Reveal release QR → Yes, show the QR.** "I have the parcel now, so I show this." Courier opens the link the QR holds (on stage: paste the link; in real life, their phone camera). The secret fills in.
   - Point out: "The secret travels in the part of the URL after the `#`. Browsers never send that to a server, so it is not in any log."
5. Courier presses **Release 5,000 sats.** Both show **Settled**.
   - In a terminal: `./scripts/demo-env.sh status`: the courier's node (`seller2`) is up by 5,000 sats.

**Say:** "That is the happy path. The money was never held by us."

## Scene 2: nobody shows up (~2 min)

1. Courier: new escrow, `Parcel #4822 (the courier never arrives)`, `3000` sats, hold **1.04 minutes** (62 seconds). Create.
2. Customer: open the link, **Accept offer**, **Pay with my wallet.** **Funds locked**, with a countdown.
3. **Do nothing.** Talk while the clock runs (about a minute):
   - "There is no refund button for the customer to press and nobody at our end to approve one. The lock simply expires."
   - "Our service cancels slightly *before* the network's own safety margin, so this works even if our server dies: Lightning itself would cancel it."
   - "The customer pays no fee for the refund; their balance goes back exactly."
4. Both windows flip to **Refunded** by themselves.

**Say:** "That refund is the proof this is a real escrow and not a payment with extra steps."

## Scene 3: a dispute, decided by an arbiter (~4 min)

1. Courier: new escrow, `Parcel #4823, signature on delivery`, `8000` sats, 30 minutes. In **Arbiter's user id** paste the arbiter's id (printed by `up`, also in `accounts.json`). Create.
   - Point out the **key fingerprint** on the page. "Both sides check this with the arbiter out of band, so nobody can swap in a key of their own."
2. Customer: open the link, **Accept**, pay. "Notice what the browser also did: it *encrypted* the secret so only the arbiter can open it, and uploaded only the ciphertext. Our server cannot read it."
3. Courier's page shows **"The arbiter hasn't confirmed yet."** "A buyer could upload junk, so the arbiter checks first."
4. **Arbiter window**: open the escrow. **Check the sealed secret**, which says it opens and matches the payment hash. **Confirm to both parties.** The courier's warning disappears.
5. "The courier delivers. The customer refuses to release." Courier: **Open a dispute…**, reason `Delivered at 14:05, signed for by the customer's wife. Customer will not release.` Everyone shows **Disputed**.
6. **Arbiter window.** The reason is quoted under **What was reported**. Note `Signed delivery note matches. Releasing to the courier.` Press **Release to the seller → Yes, pay the seller.** All three show **Settled**, and the note is in everyone's History.
   - Point out: the two decision buttons are the same weight on purpose, and the decision is recorded under the arbiter's name.

**Say:** "The arbiter decided, but they never held the money, and they could only decide between two outcomes: pay the courier or refund the customer. If they take too long, the deadline refunds the customer."

## The honest limits (say these before anyone asks)

- **Everything here is on a local test network** (regtest) with a stand-in wallet. It has not yet been tried with a real phone wallet on a public network, and a key open question is whether real wallets hold a payment pending for hours.
- **The buyer holds the power** in the plain version. They can withhold the secret even after receiving the goods; the cost to them is nothing but the refund. Delivery, where the recipient must be present, and an arbiter are the mitigations.
- **An arbiter is someone you trust**: they can open the secret, so a dishonest one could leak it. The product doesn't pick or pay arbiters.
- **Revealing the secret shows the buyer chose to release, not that the work was good.**
- **Holds are capped at 6 hours** by how long Lightning can safely hold a payment.
- **The courier needs a Lightning node that supports hold invoices.** Sellers without one use the service's shared node, which means the service receives their sats and owes them: that is labelled *custodial* on screen, and can be switched off.

## Questions you will probably get

- **"Who holds the money?"** Nobody: it is locked on the Lightning network. After release it is on the courier's own node.
- **"What if your server goes down?"** The escrow keeps its state in a database and reconciles with the node on restart (tested against a real node). If it is down at the deadline, Lightning cancels the payment itself shortly before it would expire.
- **"What if the courier's node is offline when the customer releases?"** The release can't settle and the customer is refunded at the deadline. This is the risk to flag to couriers.
- **"Can the arbiter steal it?"** They can only choose pay-courier or refund-customer; they can't send it anywhere else. Their only real power is to leak the secret.
- **"Why not just use escrow.com?"** Fees, custody, and they can freeze it. Here a lock on the network does the holding.
- **"Why isn't there a fiat price?"** Out of scope for now; amounts are in sats.

## If something goes wrong

| Symptom | Fix |
|---|---|
| Header says **Node unavailable**, or an action says the node is unavailable | `./scripts/demo-env.sh refresh` (regtest only mines on demand, and LND reports "not synced" when the newest block is over two hours old) |
| Payment fails with `NO_ROUTE` or `INSUFFICIENT_BALANCE` | `./scripts/demo-env.sh up` again (it re-links the nodes) |
| No **Pay with my wallet** button | `./scripts/demo-env.sh down && ./scripts/demo-env.sh up` (the web app has to start with the demo wallet enabled) |
| A window shows "not found" | You are signed in as the wrong person for that link; check the token |
| Arbiter says "This browser doesn't hold your private key" | Paste `.demo/arbiter-key.json` into *Restore* on the Arbiter page |
| Everything is a mess | `./scripts/demo-env.sh reset && ./scripts/demo-env.sh up` for a clean cast and database |
| Containers can't reach each other (LND never syncs) | `sudo sysctl -w net.bridge.bridge-nf-call-iptables=0`, then `up` again (`up` tries this itself) |

## Afterwards

`./scripts/demo-env.sh down` stops the server, wallet and web app. Add `docker compose -f regtest/docker-compose.yml down` to stop the Lightning network too, or `down -v` to wipe it.
