# A signet dry run

Rehearse the whole product on a public test network with free coins before using real money: an escrow that locks and releases, a buyer paying in naira, and a seller cashing out. Signet coins are worthless, so nothing here can cost you anything.

The Lightning side is **real**: payments travel between real nodes on a public test network. The naira side is **pretend**, played by the built-in demo provider, which takes "I've sent the transfer" on trust. It refuses to start on mainnet. It does not test Bitnob or any real bank.

This guide works with any signet nodes you can reach from the internet. For where to run them, see [`deploy-render.md`](deploy-render.md). It assumes the API and web app are already running on Render.

## What you need

| | |
|---|---|
| **An escrow node** | The shared node. Holds the hold invoices and, after release, the sellers' balances. |
| **A ramp node** | Plays the bank. Pays escrows when a buyer "sends naira", and receives sats when a seller cashes out. |
| **A channel between them** | With sats on **both** sides (see step 3). |
| **Three or four accounts** | A seller, a buyer, and a second buyer for the refund test. Make them in the web app. |

Both nodes must run LND on **signet** and be reachable by the API over gRPC (`https://host:10009`).

## Step 1: check the nodes can do the job

Before anything else, on the **escrow node**, check it allows hold invoices and custom keys. Some managed providers restrict both.

```bash
# A hold invoice: any 32-byte hash will do for the check.
lncli --network=signet addholdinvoice 0101010101010101010101010101010101010101010101010101010101010101 --amt 1000
lncli --network=signet cancelinvoice 0101010101010101010101010101010101010101010101010101010101010101
# A custom key:
lncli --network=signet bakemacaroon invoices:read invoices:write info:read --save_to=/tmp/check.macaroon
```

If either command is refused, that provider cannot run escrows: pick another, or run the node yourself.

## Step 2: get test coins

1. Get an on-chain address from each node: `lncli --network=signet newaddress p2wkh`.
2. Paste it into a signet faucet, for example [signetfaucet.com](https://signetfaucet.com/) (or the [Mutinynet](https://nobsbitcoin.com/mutinynet) faucet if you use that network). Faucets come and go: check one is up.
3. Wait for the deposit to confirm: `lncli --network=signet walletbalance`. About 500,000 sats on the ramp node is plenty; the escrow node needs a little for channel fees.

## Step 3: open the channel, with sats on both sides

The ramp node pays escrows (so it needs sats on its side), and the escrow node pays cash-outs back (so it needs sats on its side too). Open the channel **from the ramp node** and push some sats across:

```bash
# on the ramp node
lncli --network=signet connect <escrow-node-pubkey>@<escrow-node-host>:9735
lncli --network=signet openchannel --node_key <escrow-node-pubkey> --local_amt 400000 --push_amt 100000
```

Wait for it to confirm and become active (`lncli --network=signet listchannels`). A channel that is not active is the most common reason for "no route" errors later.

## Step 4: make the keys

Never give the API an admin macaroon. On the **escrow node**:

```bash
lncli --network=signet bakemacaroon invoices:read invoices:write info:read --save_to=toshi.macaroon
lncli --network=signet bakemacaroon offchain:read offchain:write invoices:read invoices:write info:read --save_to=payout.macaroon
```

On the **ramp node** (the same permissions as the payout key; it only ever plays the bank):

```bash
lncli --network=signet bakemacaroon offchain:read offchain:write invoices:read invoices:write info:read --save_to=ramp.macaroon
```

Encode each file and each node's certificate as one line, for pasting into Render:

```bash
base64 -w0 ~/.lnd/tls.cert     # one per node
base64 -w0 toshi.macaroon
base64 -w0 payout.macaroon
base64 -w0 ramp.macaroon
```

## Step 5: set the API's settings on Render

On the **API service**, Environment. Keep what you already have and set or add:

| Setting | Value |
|---|---|
| `SHARED_NODE` | `on` |
| `LND_ADDR` | the escrow node's `https://host:10009` |
| `LND_TLS_CERT_B64` | escrow node certificate |
| `LND_MACAROON_B64` | `toshi.macaroon` |
| `PAYOUT_MACAROON_B64` | `payout.macaroon`, so sellers get balances |
| `RAMP_PROVIDER` | `demo` |
| `RAMP_LND_ADDR` | the ramp node's `https://host:10009` |
| `RAMP_LND_TLS_CERT_B64` | ramp node certificate |
| `RAMP_LND_MACAROON_B64` | `ramp.macaroon` |
| `FIAT_PROVIDER` | `fixed:USD=65000,NGN=97500000` (a labelled demo price; signet has no market) |
| `PIN_COOLOFF_HOURS` | `0` for this rehearsal (normally 24) |
| `NAIRA_HOLD_HOURS` | `0` for this rehearsal (normally 24) |

Save, and wait for the deploy. Then check:

1. `https://<api>/health` says `"node":"up"`. If it says `down`, the API cannot reach the escrow node: check the address, certificate and key.
2. The deploy log says `RAMP_PROVIDER=demo: naira payments are pretend (test networks only)`. If it exits saying the demo only runs against a test network, the ramp node is not on signet.

Put `PIN_COOLOFF_HOURS` and `NAIRA_HOLD_HOURS` back to `24` when you finish.

## Step 6: the tests

You need to pay Lightning invoices from a signet wallet for test 1. The ramp node can do that: `lncli --network=signet payinvoice <invoice>` (or use any signet wallet). Open the web app in separate browser profiles so accounts do not share a sign-in.

### Test 1: a plain escrow, paid over Lightning

1. **Seller:** make an escrow for 5,000 sats. The page says funds go to the shared node.
2. **Buyer:** open the link, accept, and pay the invoice from a signet wallet.
3. Both pages should say **Funds locked**.
4. **Buyer:** reveal the release QR. **Seller:** open its link and release.
5. **Seller:** **Balance** shows the sats. This proves real hold invoices, locking and settling.

### Test 2: a buyer pays in naira (the on-ramp)

1. **Seller:** make an escrow. **Buyer:** open the link and accept.
2. **Buyer:** choose **Pay in naira**, pick a bank and enter any 10-digit account number. A red "Demo bank, no real money" notice shows.
3. **Buyer:** press **I've sent the transfer (demo)**.
4. The ramp node pays the escrow's hold invoice over signet. Both pages should flip to **Funds locked**.
5. Release as in test 1. The seller's balance goes up.

### Test 3: the naira refund

1. Repeat test 2 with a short hold (for example 2 minutes) and do **not** release.
2. At the deadline the escrow is refunded and the (pretend) naira goes back to the account the buyer paid **from**, not one they typed afterwards.

### Test 4: a seller cashes out (the off-ramp)

1. **Seller:** with a balance from test 1 or 2, open **Balance** and set a withdrawal PIN (it works at once with `PIN_COOLOFF_HOURS=0`).
2. Choose the **Bank account** tab, pick a bank, enter an account number, an amount of at least 1,000 sats, and the PIN.
3. The escrow node pays the ramp node's invoice over signet. The history shows the withdrawal go from *Sending* to *Sent*, with the pretend naira amount.
4. Try a wrong PIN five times to see it lock, and try an amount above the balance to see the error.

## If something fails

| What you see | Likely cause |
|---|---|
| `/health` says node `down` | Wrong `LND_ADDR`, certificate or key, or the node is offline. |
| Paying an escrow or cash-out fails with "no route" or "insufficient" | The channel is not active, or has no sats on the paying side. Check `listchannels`, and push or pay some sats across. |
| The server exits at start naming a network | `RAMP_LND_ADDR` points at a mainnet node. That is the guard working. |
| "Naira is unavailable" or no price | `FIAT_PROVIDER` is missing the `NGN` price. |
| Withdrawal says the PIN is cooling off | `PIN_COOLOFF_HOURS` is still 24. |
| Cash-out refused as too small | The minimum is 1,000 sats. |
| Naira-paid sats will not withdraw | `NAIRA_HOLD_HOURS` is still 24: sats from a naira payment mature first. |

## When you are done

- Put `PIN_COOLOFF_HOURS` and `NAIRA_HOLD_HOURS` back to `24`.
- Before mainnet: set `RAMP_PROVIDER` to `off` (the demo refuses mainnet anyway), point the `LND_*` settings at your **mainnet** node, and start with tiny limits (see [`deploy-render.md`](deploy-render.md)).
- Everything above is a rehearsal of the Lightning side only. A real naira provider, such as Bitnob, needs its own integration and its own tests.
