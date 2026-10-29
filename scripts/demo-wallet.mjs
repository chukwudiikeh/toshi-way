#!/usr/bin/env node
// A stand-in Lightning wallet for demos on the local regtest network.
//
// There is no phone wallet on regtest, so this small local server pays invoices from the "buyer"
// LND node. The web app (started with NEXT_PUBLIC_DEMO_WALLET_URL) shows a "Pay with my wallet"
// button that posts the invoice here, so nobody has to type `lncli` during a demo.
//
// It only listens on localhost, only accepts regtest invoices (lnbcrt...), and never builds a shell
// command from the input. It is a demo tool: never run it against anything but regtest.

import { execFile } from "node:child_process";
import { createServer } from "node:http";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const port = Number(process.env.DEMO_WALLET_PORT ?? 3100);
const origins = (process.env.DEMO_WALLET_ORIGINS ?? "http://localhost:3001,http://127.0.0.1:3001").split(",");
const REGTEST_INVOICE = /^lnbcrt[0-9a-z]{20,4000}$/;

const server = createServer((req, res) => {
  const origin = req.headers.origin;
  if (origin && origins.includes(origin)) {
    res.setHeader("Access-Control-Allow-Origin", origin);
    res.setHeader("Access-Control-Allow-Headers", "content-type");
    res.setHeader("Access-Control-Allow-Methods", "POST, OPTIONS");
    res.setHeader("Vary", "Origin");
  }
  const reply = (status, body) => {
    res.writeHead(status, { "content-type": "application/json" });
    res.end(JSON.stringify(body));
  };
  if (req.method === "OPTIONS") return reply(204, {});
  if (req.method === "GET" && req.url === "/health") return reply(200, { ok: true, wallet: "demo (regtest buyer node)" });
  if (req.method !== "POST" || req.url !== "/pay") return reply(404, { error: "not found" });
  // Only the demo web app may ask this wallet to pay.
  if (origin && !origins.includes(origin)) return reply(403, { error: "origin not allowed" });

  let raw = "";
  req.on("data", (c) => {
    raw += c;
    if (raw.length > 8192) req.destroy();
  });
  req.on("end", () => {
    let invoice;
    try {
      invoice = String(JSON.parse(raw).invoice ?? "").trim().toLowerCase();
    } catch {
      return reply(400, { error: "send JSON like {\"invoice\": \"lnbcrt...\"}" });
    }
    if (!REGTEST_INVOICE.test(invoice)) return reply(400, { error: "that is not a regtest invoice" });

    // Arguments go as an array: the invoice is never interpreted by a shell. lncli blocks while a
    // hold invoice is held, which is correct, so the request is acknowledged without waiting.
    const args = [
      "compose", "-f", resolve(repo, "regtest/docker-compose.yml"),
      "exec", "-T", "buyer", "lncli", "--network=regtest", "payinvoice", "--force", invoice,
    ];
    const child = execFile("docker", args, { timeout: 15 * 60_000 }, (err, _out, stderr) => {
      if (err) console.error(`payment ${invoice.slice(0, 16)}... ended: ${String(stderr || err.message).split("\n")[0]}`);
    });
    child.unref?.();
    console.log(`paying ${invoice.slice(0, 16)}... from the buyer node`);
    reply(202, { accepted: true });
  });
});

server.listen(port, "127.0.0.1", () => console.log(`demo wallet listening on http://127.0.0.1:${port}`));
