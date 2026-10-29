// Both demo paths through the browser, modelled on scripts/demo.sh.
//
//   ./regtest/setup.sh
//   OPERATOR_TOKEN=... cargo run -p escrow-api --bin toshi-server
//   (cd web && npm run dev -- -p 3001)
//   (cd web && npm run e2e)
//
// The buyer's invoice is paid from the buyer node with lncli, as a developer would by hand.

import { execFile, spawn } from "node:child_process";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { promisify } from "node:util";
import { expect, test, type Browser, type BrowserContext, type Page } from "@playwright/test";

const REPO = resolve(__dirname, "../..");
const COMPOSE = ["compose", "-f", resolve(REPO, "regtest/docker-compose.yml")];

/** Regtest only mines on demand; LND reports "not synced" after two hours without a block. */
async function freshChainTip() {
  const run = promisify(execFile);
  const cli = [...COMPOSE, "exec", "-T", "bitcoind", "bitcoin-cli", "-regtest", "-rpcuser=toshi", "-rpcpassword=toshi"];
  await run("docker", [...cli, "loadwallet", "miner"]).catch(() => {});
  await run("docker", [...cli, "-rpcwallet=miner", "-generate=1"]);
}

/** Pay from the buyer node. lncli blocks while the payment is held, so don't wait for it. */
function payInvoice(invoice: string) {
  const child = spawn("docker", [...COMPOSE, "exec", "-T", "buyer", "lncli", "--network=regtest", "payinvoice", "--force", invoice], {
    stdio: "ignore",
    detached: true,
  });
  child.unref();
}

const stateBadge = (page: Page, label: string) => page.locator("span[title]", { hasText: label });

type Party = { context: BrowserContext; page: Page; id: string; requests: { url: string; body: string | null }[] };

/** A fresh browser profile with a new account, recording every request it sends. */
async function newParty(browser: Browser): Promise<Party> {
  const context = await browser.newContext();
  const page = await context.newPage();
  const requests: Party["requests"] = [];
  page.on("request", (r) => requests.push({ url: r.url(), body: r.postData() }));

  await page.goto("/");
  await page.getByRole("button", { name: "Create account" }).click();
  await expect(page.getByText("Save your token now")).toBeVisible();
  const id = (await page.locator("code").nth(1).textContent())!.trim();
  expect(id).toMatch(/^u_/);
  await page.getByLabel("I have saved my token somewhere safe").check();
  await page.getByRole("button", { name: "Continue" }).click();
  await expect(page.getByRole("heading", { name: "My escrows" })).toBeVisible();
  return { context, page, id, requests };
}

async function createEscrow(seller: Party, buyerId: string, description: string, hold: [string, "minutes" | "hours"]) {
  const { page } = seller;
  await page.goto("/escrows/new");
  await page.getByLabel("Buyer's user id").fill(buyerId);
  await page.getByLabel("Amount (sats)").fill("1000");
  await page.getByLabel("Description").fill(description);
  await page.getByLabel("Hold time").fill(hold[0]);
  await page.locator("select").selectOption(hold[1]);
  await page.getByRole("button", { name: "Create escrow" }).click();
  await page.waitForURL(/\/escrows\/[0-9a-f-]{36}$/);
  return page.url().split("/").pop()!;
}

/** Buyer accepts and pays; returns once both sides show the funds locked. */
async function acceptAndPay(buyer: Party, seller: Party, id: string) {
  await buyer.page.goto(`/escrows/${id}`);
  await buyer.page.getByRole("button", { name: /Accept offer/ }).click();
  await expect(buyer.page.getByRole("button", { name: "Copy invoice" })).toBeVisible();
  const invoice = (await buyer.page.locator("code", { hasText: /^lnbcrt/ }).first().textContent())!.trim();
  await expect(buyer.page.getByText("Pay within")).toBeVisible();
  payInvoice(invoice);
  for (const p of [buyer.page, seller.page]) await expect(stateBadge(p, "Funds locked")).toBeVisible();
}

test.beforeAll(freshChainTip);

test("happy path: lock, hand over the release QR link, settle", async ({ browser }) => {
  const seller = await newParty(browser);
  const buyer = await newParty(browser);
  const id = await createEscrow(seller, buyer.id, "e2e parcel", ["30", "minutes"]);
  await expect(seller.page.getByText("Link for the buyer")).toBeVisible();

  await acceptAndPay(buyer, seller, id);
  await expect(seller.page.getByText("Funds are locked: you can start the work.")).toBeVisible();

  // Once paid, the buyer has no cancel button (the API would answer 403 anyway).
  await expect(buyer.page.getByRole("button", { name: /^Cancel/ })).toHaveCount(0);

  // A wrong secret is refused and changes nothing.
  await seller.page.getByLabel("Release secret").fill("ab".repeat(32));
  await seller.page.getByRole("button", { name: /^Release 1,000 sats/ }).click();
  await expect(seller.page.getByText("That secret doesn't match this escrow.")).toBeVisible();
  // A short code is refused before it is sent.
  await seller.page.getByLabel("Release secret").fill("123456");
  await seller.page.getByRole("button", { name: /^Release 1,000 sats/ }).click();
  await expect(seller.page.getByText("exactly 64 hex characters")).toBeVisible();
  await expect(stateBadge(seller.page, "Funds locked")).toBeVisible();

  // The buyer reveals the release QR; the link it encodes is what the seller's camera would open.
  await buyer.page.getByRole("button", { name: "Reveal release QR" }).click();
  await buyer.page.getByRole("button", { name: "Yes, show the QR" }).click();
  await expect(buyer.page.getByRole("img", { name: "Release QR" })).toBeVisible();
  const secret = await buyer.page.evaluate((id) => localStorage.getItem(`toshi.secret.${id}`), id);
  expect(secret).toMatch(/^[0-9a-f]{64}$/);

  await seller.page.goto(`/escrows/${id}#release=${secret}`);
  await expect(seller.page.getByText("The secret was filled in from the release link.")).toBeVisible();
  expect(new URL(seller.page.url()).hash).toBe(""); // removed from the address bar at once
  await expect(seller.page.getByLabel("Release secret")).toHaveValue(secret!);
  await seller.page.getByRole("button", { name: /^Release 1,000 sats/ }).click();

  for (const p of [seller.page, buyer.page]) await expect(stateBadge(p, "Settled")).toBeVisible();
  await expect(seller.page.getByText("Done. You were paid.")).toBeVisible();

  // The secret left the browser exactly once: in the seller's release call.
  const carrying = [...seller.requests, ...buyer.requests].filter(
    (r) => r.url.includes(secret!) || r.body?.includes(secret!),
  );
  expect(carrying).toHaveLength(1);
  expect(carrying[0].url).toMatch(new RegExp(`/api/v1/escrows/${id}/release$`));

  // Someone who isn't a party gets "not found", not the escrow.
  const stranger = await newParty(browser);
  await stranger.page.goto(`/escrows/${id}`);
  await expect(stranger.page.getByText("Escrow not found")).toBeVisible();
});

test("refund path: a 62 second hold is refunded with nobody acting", async ({ browser }) => {
  const seller = await newParty(browser);
  const buyer = await newParty(browser);
  const id = await createEscrow(seller, buyer.id, "e2e refund", ["1.04", "minutes"]); // 62 s
  await acceptAndPay(buyer, seller, id);
  for (const p of [seller.page, buyer.page]) {
    await expect(stateBadge(p, "Refunded")).toBeVisible({ timeout: 120_000 });
  }
  await expect(buyer.page.getByText("Your payment was cancelled and the funds are back in your wallet.")).toBeVisible();
});

test("an offer nobody has accepted can be withdrawn by the seller or declined by the buyer", async ({ browser }) => {
  const seller = await newParty(browser);
  const buyer = await newParty(browser);

  const withdrawn = await createEscrow(seller, buyer.id, "e2e withdrawn", ["30", "minutes"]);
  await seller.page.getByRole("button", { name: "Withdraw offer" }).click();
  await seller.page.getByRole("button", { name: "Withdraw offer" }).last().click();
  await expect(stateBadge(seller.page, "Ended unpaid")).toBeVisible();
  await buyer.page.goto(`/escrows/${withdrawn}`);
  await expect(stateBadge(buyer.page, "Ended unpaid")).toBeVisible();
  await expect(buyer.page.getByRole("button", { name: /Accept offer/ })).toHaveCount(0);

  const declined = await createEscrow(seller, buyer.id, "e2e declined", ["30", "minutes"]);
  await buyer.page.goto(`/escrows/${declined}`);
  await buyer.page.getByRole("button", { name: "Decline" }).click();
  await buyer.page.getByRole("button", { name: "Yes, decline" }).click();
  await expect(stateBadge(buyer.page, "Ended unpaid")).toBeVisible();
  await expect(stateBadge(seller.page, "Ended unpaid")).toBeVisible();
});

test("signing in with a token finds the account's id and role", async ({ browser }) => {
  const user = await newParty(browser);
  const token = (await user.page.evaluate(() => localStorage.getItem("toshi.session")))!;
  const fresh = await browser.newContext();
  const page = await fresh.newPage();
  await page.goto("/");
  await page.getByLabel("Token").fill(JSON.parse(token).token);
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByRole("heading", { name: "My escrows" })).toBeVisible();
  await expect(page.getByText(user.id)).toBeVisible();
});

test("open offer: a link anyone can open; a brand-new visitor becomes the buyer", async ({ browser }) => {
  const seller = await newParty(browser);
  const { page } = seller;

  await page.goto("/escrows/new");
  await page.getByLabel("Amount (sats)").fill("1000");
  await page.getByLabel("Description").fill("e2e open offer");
  await page.getByLabel("Hold time").fill("30");
  await page.locator("select").selectOption("minutes");
  await page.getByRole("button", { name: "Create escrow" }).click(); // no buyer id
  await page.waitForURL(/\/escrows\/[0-9a-f-]{36}$/);
  const id = page.url().split("/").pop()!;
  await expect(page.getByText("the first person to open the link and accept becomes the buyer")).toBeVisible();

  // A visitor with no account follows the link, creates one on the spot, and lands on the offer.
  const context = await browser.newContext();
  const visitor = await context.newPage();
  await visitor.goto(`/escrows/${id}`);
  await visitor.getByRole("button", { name: "Create account" }).click();
  await visitor.getByLabel("I have saved my token somewhere safe").check();
  await visitor.getByRole("button", { name: "Continue" }).click();
  await expect(visitor.getByText("e2e open offer")).toBeVisible();
  await expect(visitor.getByRole("button", { name: "Decline" })).toHaveCount(0);

  await visitor.getByRole("button", { name: /Accept offer/ }).click();
  await expect(visitor.getByRole("button", { name: "Copy invoice" })).toBeVisible();
  await expect(stateBadge(page, "Awaiting payment")).toBeVisible();

  // Now it belongs to the visitor: a third person gets "not found".
  const stranger = await newParty(browser);
  await stranger.page.goto(`/escrows/${id}`);
  await expect(stranger.page.getByText("Escrow not found")).toBeVisible();
});

test("one-click WebLN payment by the buyer and camera scan of the release QR by the seller", async ({ browser }) => {
  const seller = await newParty(browser);
  const buyer = await newParty(browser);

  // The buyer's browser has a WebLN wallet; here it pays through the buyer node like a developer would.
  await buyer.page.exposeFunction("__payFromWallet", (invoice: string) => payInvoice(invoice));
  await buyer.page.addInitScript(() => {
    (window as unknown as { webln: unknown }).webln = {
      enable: async () => {},
      sendPayment: (invoice: string) => {
        (window as unknown as { __payFromWallet: (i: string) => void }).__payFromWallet(invoice);
        return new Promise(() => {}); // a hold invoice keeps the payment pending while locked
      },
    };
  });

  // The seller's browser has a camera that "sees" whatever the test puts in __scanText.
  await seller.page.addInitScript(() => {
    const w = window as unknown as Record<string, unknown>;
    w.__scanText = "";
    w.BarcodeDetector = class {
      async detect() {
        return w.__scanText ? [{ rawValue: w.__scanText }] : [];
      }
    };
    const canvas = document.createElement("canvas");
    canvas.width = canvas.height = 64;
    navigator.mediaDevices.getUserMedia = async () => canvas.captureStream(5);
  });

  const id = await createEscrow(seller, buyer.id, "e2e webln and scan", ["30", "minutes"]);
  await buyer.page.goto(`/escrows/${id}`);
  await buyer.page.getByRole("button", { name: /Accept offer/ }).click();
  await buyer.page.getByRole("button", { name: /with my wallet/ }).click();
  // The page makes no claim of its own: it flips to LOCKED only when the API reports it.
  for (const p of [buyer.page, seller.page]) await expect(stateBadge(p, "Funds locked")).toBeVisible();

  const secret = await buyer.page.evaluate((id) => localStorage.getItem(`toshi.secret.${id}`), id);
  await seller.page.getByRole("button", { name: "Scan the buyer's QR" }).click();
  // Scanning another escrow's code is refused; this escrow's link fills the field but does not release.
  await seller.page.evaluate(
    (t) => ((window as unknown as { __scanText: string }).__scanText = t),
    `http://localhost:3001/escrows/not-this-one#release=${secret}`,
  );
  await expect(seller.page.getByText("That release code is for a different escrow.")).toBeVisible();
  await seller.page.evaluate(
    (t) => ((window as unknown as { __scanText: string }).__scanText = t),
    `http://localhost:3001/escrows/${id}#release=${secret}`,
  );
  await expect(seller.page.getByLabel("Release secret")).toHaveValue(secret!);
  await expect(stateBadge(seller.page, "Funds locked")).toBeVisible();

  await seller.page.getByRole("button", { name: /^Release 1,000 sats/ }).click();
  for (const p of [seller.page, buyer.page]) await expect(stateBadge(p, "Settled")).toBeVisible();
});


/** Spendable sats on a regtest node's channels (what the seller would see in their own wallet). */
async function channelBalance(node: string): Promise<number> {
  const run = promisify(execFile);
  const { stdout } = await run("docker", [...COMPOSE, "exec", "-T", node, "lncli", "--network=regtest", "channelbalance"]);
  return Number(JSON.parse(stdout).local_balance.sat);
}

const creds = (node: string, file: string) => resolve(REPO, "regtest/creds", node, file);

test("a seller connects their own node and is paid on it", async ({ browser }) => {
  const seller = await newParty(browser);
  const buyer = await newParty(browser);
  const { page } = seller;

  // Before connecting a node the seller is told the service's shared node would hold the money.
  await page.goto("/escrows/new");
  await expect(page.getByText("this escrow will use the service's shared node")).toBeVisible();

  // Wrong credentials are refused and nothing is saved: seller's macaroon against seller2's address.
  await page.getByRole("link", { name: "My node" }).click();
  await expect(page.getByText("No node connected.").first()).toBeVisible();
  await page.getByLabel("Node address").fill("https://localhost:10004");
  await page.getByLabel("TLS certificate").fill(readFileSync(creds("seller2", "tls.cert"), "utf8"));
  await page.getByLabel("Invoice macaroon (hex)").fill(readFileSync(creds("seller", "toshi.macaroon")).toString("hex"));
  await page.getByRole("button", { name: "Check and save" }).click();
  await expect(page.getByText(/could not|refused|did not pass/i).first()).toBeVisible();
  await expect(page.getByText("Your node is connected.")).toHaveCount(0);

  // Right credentials: connected.
  await page.getByLabel("Invoice macaroon (hex)").fill(readFileSync(creds("seller2", "toshi.macaroon")).toString("hex"));
  await page.getByRole("button", { name: "Check and save" }).click();
  await expect(page.getByText("Your node is connected.")).toBeVisible();
  await expect(page.getByText("https://localhost:10004")).toBeVisible();
  // The credentials are not kept in the form, and the page never shows the macaroon back.
  await expect(page.getByLabel("Invoice macaroon (hex)")).toHaveValue("");

  // An escrow now lives on the seller's own node.
  const before = await channelBalance("seller2");
  const id = await createEscrow(seller, buyer.id, "e2e own node", ["30", "minutes"]);
  await expect(page.getByText("The seller's own node")).toBeVisible();
  await acceptAndPay(buyer, seller, id);
  await buyer.page.getByRole("button", { name: "Reveal release QR" }).click();
  await buyer.page.getByRole("button", { name: "Yes, show the QR" }).click();
  const secret = await buyer.page.evaluate((id) => localStorage.getItem(`toshi.secret.${id}`), id);
  await page.goto(`/escrows/${id}#release=${secret}`);
  await page.getByRole("button", { name: /^Release 1,000 sats/ }).click();
  for (const p of [page, buyer.page]) await expect(stateBadge(p, "Settled")).toBeVisible();

  // The sats arrived on the seller's own node, not the service's.
  await expect.poll(() => channelBalance("seller2"), { timeout: 20_000 }).toBe(before + 1000);

  // While an escrow is in flight the node is locked in; once it is over it can be changed.
  await page.goto("/node");
  await expect(page.getByRole("button", { name: "Disconnect" })).toBeVisible();
});
