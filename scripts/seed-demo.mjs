#!/usr/bin/env node
// Create the demo cast against a running toshi-server, once, and remember them in .demo/.
//
//   courier   a seller who runs their OWN Lightning node (seller2), so sats land with them directly
//   customer  the buyer
//   arbiter   a trusted referee with a ready key (restore .demo/arbiter-key.json on the Arbiter page)
//
// Tokens are shown once by the API, so they are saved to .demo/accounts.json (gitignored). Running
// this again reuses them while they still work.

import { webcrypto as crypto } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const dir = resolve(repo, ".demo");
const API = process.env.API ?? "http://127.0.0.1:3000";
mkdirSync(dir, { recursive: true });

async function api(method, path, token, body) {
  const res = await fetch(API + path, {
    method,
    headers: { ...(token ? { authorization: `Bearer ${token}` } : {}), ...(body ? { "content-type": "application/json" } : {}) },
    body: body ? JSON.stringify(body) : undefined,
  });
  const text = await res.text();
  return { status: res.status, body: text ? JSON.parse(text) : null };
}

const file = resolve(dir, "accounts.json");
let accounts = existsSync(file) ? JSON.parse(readFileSync(file, "utf8")) : {};

// Reuse an account only if its token still works against this database.
async function ensure(name) {
  const known = accounts[name];
  if (known) {
    const me = await api("GET", "/api/v1/me", known.token);
    if (me.status === 200 && me.body.id === known.id) return known;
  }
  const made = await api("POST", "/api/v1/users");
  if (made.status !== 201) throw new Error(`could not create ${name}: ${JSON.stringify(made.body)}`);
  return { id: made.body.id, token: made.body.token };
}

for (const name of ["courier", "customer", "arbiter"]) accounts[name] = await ensure(name);

// The courier brings their own node, so the demo shows money landing with the seller, not the service.
const node = await api("GET", "/api/v1/me/node", accounts.courier.token);
if (node.status === 200 && node.body.mode !== "own") {
  if (!node.body.own_nodes_enabled) throw new Error("the server has no NODE_CREDENTIAL_KEY, so sellers cannot connect nodes");
  const put = await api("PUT", "/api/v1/me/node", accounts.courier.token, {
    address: "https://localhost:10004",
    tls_cert: readFileSync(resolve(repo, "regtest/creds/seller2/tls.cert"), "utf8"),
    macaroon: readFileSync(resolve(repo, "regtest/creds/seller2/toshi.macaroon")).toString("hex"),
  });
  if (put.status !== 200) throw new Error(`could not connect the courier's node: ${JSON.stringify(put.body)}`);
}

// The arbiter's key pair is made here, registered, and saved as a backup the Arbiter page can restore.
const registered = await api("GET", "/api/v1/me/arbiter-key", accounts.arbiter.token);
const keyFile = resolve(dir, "arbiter-key.json");
if (!registered.body.public_key || !existsSync(keyFile)) {
  const pair = await crypto.subtle.generateKey({ name: "ECDH", namedCurve: "P-256" }, true, ["deriveBits"]);
  const spki = Buffer.from(await crypto.subtle.exportKey("spki", pair.publicKey)).toString("base64");
  const jwk = await crypto.subtle.exportKey("jwk", pair.privateKey);
  const put = await api("PUT", "/api/v1/me/arbiter-key", accounts.arbiter.token, { public_key: spki });
  if (put.status !== 200) throw new Error(`could not register the arbiter key: ${JSON.stringify(put.body)}`);
  writeFileSync(keyFile, JSON.stringify(jwk), { mode: 0o600 });
}
accounts.arbiter.fingerprint = (await api("GET", "/api/v1/me/arbiter-key", accounts.arbiter.token)).body.fingerprint;

writeFileSync(file, JSON.stringify(accounts, null, 2), { mode: 0o600 });

const row = (label, a, note) => console.log(`  ${label.padEnd(9)} ${a.id}  ${note}`);
console.log("Demo cast (tokens are in .demo/accounts.json):");
row("courier", accounts.courier, "seller, own Lightning node");
row("customer", accounts.customer, "buyer");
row("arbiter", accounts.arbiter, `referee, key ${accounts.arbiter.fingerprint.slice(0, 19)}...`);
