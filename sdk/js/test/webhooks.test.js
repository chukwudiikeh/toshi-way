import assert from "node:assert/strict";
import { createHmac } from "node:crypto";
import { test } from "node:test";
import { verifyWebhook, WebhookSignatureError } from "../src/index.js";

const secret = "whsec_test";
const sign = (t, body, key = secret) => createHmac("sha256", key).update(`${t}.${body}`).digest("hex");
const body = '{"id":"evt_7","type":"escrow.locked","created_at":1700000000,"data":{"state":"LOCKED"}}';
const T = 1_700_000_000;
const header = (t = T, b = body) => `t=${t},v1=${sign(t, b)}`;
const reason = (fn) => {
  try {
    fn();
  } catch (e) {
    assert.ok(e instanceof WebhookSignatureError);
    return e.reason;
  }
  assert.fail("should have thrown");
};

test("matches the value the server's own test pins, so the two sides agree", () => {
  // Computed by the Rust side and by Python independently: key whsec_test, t=1700000000, this body.
  assert.equal(sign(1700000000, '{"id":"evt_1"}'), "c89214b5b5da833daed6f0b8c5bb6bd58cea9022bd80ccc78230f3942d632925");
});

test("a good message is accepted and parsed, from a string or from bytes", () => {
  const event = verifyWebhook({ payload: body, signature: header(), secret, now: T + 10 });
  assert.equal(event.id, "evt_7");
  assert.equal(event.data.state, "LOCKED");
  assert.equal(verifyWebhook({ payload: Buffer.from(body), signature: header(), secret, now: T }).type, "escrow.locked");
});

test("a changed body, secret or timestamp is refused", () => {
  assert.equal(reason(() => verifyWebhook({ payload: body.replace("LOCKED", "SETTLED"), signature: header(), secret, now: T })), "the signature does not match");
  assert.equal(reason(() => verifyWebhook({ payload: body, signature: header(), secret: "whsec_other", now: T })), "the signature does not match");
  assert.equal(reason(() => verifyWebhook({ payload: body, signature: `t=${T + 1},v1=${sign(T, body)}`, secret, now: T })), "the signature does not match");
  // Parsing and re-serialising the body changes its text, which is why the raw body is required.
  const reserialised = JSON.stringify(JSON.parse(body), null, 1);
  assert.equal(reason(() => verifyWebhook({ payload: reserialised, signature: header(), secret, now: T })), "the signature does not match");
});

test("an old message is refused even though its signature is genuine", () => {
  assert.match(reason(() => verifyWebhook({ payload: body, signature: header(), secret, now: T + 301 })), /too old/);
  assert.match(reason(() => verifyWebhook({ payload: body, signature: header(), secret, now: T - 301 })), /too old/);
  assert.equal(verifyWebhook({ payload: body, signature: header(), secret, now: T + 300 }).id, "evt_7", "the edge is allowed");
  assert.equal(verifyWebhook({ payload: body, signature: header(), secret, now: T + 5000, toleranceSecs: 10_000 }).id, "evt_7");
});

test("a missing or malformed header is refused", () => {
  for (const signature of [undefined, null, "", "garbage", `v1=${sign(T, body)}`, `t=${T}`, `t=abc,v1=${sign(T, body)}`, `t=${T},v1=zz`, `t=${T},v1=${sign(T, body).slice(2)}`]) {
    assert.match(reason(() => verifyWebhook({ payload: body, signature, secret, now: T })), /malformed|does not match/, String(signature));
  }
  assert.match(reason(() => verifyWebhook({ payload: body, signature: header(), secret: "", now: T })), /no signing secret/);
});

test("during a secret change, any one of several signatures may match", () => {
  const two = `t=${T},v1=${sign(T, body, "whsec_old")},v1=${sign(T, body)}`;
  assert.equal(verifyWebhook({ payload: body, signature: two, secret, now: T }).id, "evt_7");
});
