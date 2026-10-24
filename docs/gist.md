PRD: Lightning HODL-Invoice Escrow

Version: 0.1 (draft)
Status: Hackathon MVP specification
Working title: HODL Escrow
Primary languages: Rust (backend) and TypeScript (frontend)
Principle: Prove one real lock-condition-settle-or-refund cycle on Lightning before building anything else.

1. Product Overview

HODL Escrow lets two parties who don't trust each other transact over Bitcoin Lightning with the payment held in escrow until a condition is met. The payer's funds are locked in flight using a Lightning HODL invoice. They are released to the seller when the payer (or recipient) hands over a secret, and they are refunded automatically if that does not happen before a deadline.

It works for any job where payment should follow a real-world outcome: a courier delivery, a freelance deliverable, or a P2P trade.

The product is a small coordination service plus a web app that wraps a real Lightning node. It does not take custody of funds in the core mode, and it does not implement a custom payment protocol.

2. Problem Statement

Pay-first and deliver-first both require trust:

If the buyer pays first, the seller can take the money and disappear.
If the seller delivers first, the buyer can take the goods and refuse to pay.

On-chain Bitcoin multisig escrow is slow and awkward for small jobs. Custodial escrow services work, but they hold the money, charge fees, and become a point of failure. Plain Lightning is fast and cheap but settles instantly, so there is no escrow window.

HODL invoices give Lightning a built-in escrow window. This project turns that primitive into a usable product: a flow that non-experts can follow, with timeouts, state tracking, and refunds handled correctly.

3. Goals and Non-Goals
3.1 MVP Goals
Create an escrow between a buyer and a seller with an agreed amount in sats and a deadline.
Lock the buyer's payment using a real HODL invoice on a Lightning test environment.
Release the payment to the seller when the buyer or recipient provides the release secret.
Refund the buyer automatically if the deadline passes without release.
Track escrow state from the Lightning node's events, never from client claims.
Provide a web interface for buyer and seller that shows the lock, the countdown, the release action, and the final outcome.
Demonstrate both the happy path and the timeout-refund path end to end.
3.2 Non-Goals for the MVP
Custody of user funds beyond what the node must hold during a payment.
Automated arbitration or a decentralized dispute system.
Fiat conversion, naira on/off-ramps, or currency custody.
A custom Lightning routing algorithm.
Holds longer than the network can safely support (days to weeks).
Support for custodial wallets as the seller side (the seller needs a node that supports hold invoices).
A token, governance, or native mobile apps.
Guaranteeing that the real-world delivery or work actually happened.
4. Users

Buyer / payer. Wants to pay safely for goods or work from someone they don't know.

Needs: create an escrow, pay one invoice, release funds when satisfied, get a refund if the job isn't done.

Seller / provider. A courier, freelancer, or trader who wants proof that payment is committed before starting work.

Needs: see that funds are locked, know the deadline, claim payment on completion.

Recipient (optional). For deliveries, the person receiving the package may be different from the buyer. They hold the release secret and hand it over at delivery.

Operator. Runs the service during the hackathon. Needs visibility into escrows, node health, and stuck states.

5. Core Concept

A Lightning payment is a chain of conditional locks (HTLCs). Each says: pay the next hop if it shows a secret P whose hash is H, before a block-height deadline. Otherwise the money goes back.

A HODL invoice makes the recipient's node receive that locked payment without revealing P straight away. While it waits, the money is committed but not spent. The node can then:

Settle by revealing P, which completes the payment, or
Cancel, which fails the HTLCs and refunds the payer.

If the node does nothing before the HTLC's expiry, the lock times out and the payer is refunded. That window is the escrow.

6. Operating Modes
Mode A: Preimage release (non-custodial) - MVP core
The buyer's side generates a random 32-byte secret P and computes H = sha256(P).
Only H is given to the seller. P stays with the buyer or recipient.
The seller's node creates a hold invoice for hash H and the agreed amount.
The buyer pays the invoice. The payment locks and reaches the seller's node as an accepted but unsettled invoice.
The seller sees the funds are committed and starts the job.
On completion, the buyer or recipient gives P to the seller.
The seller's node settles the invoice with P and the seller is paid.
If P is never provided, the invoice is cancelled or times out and the buyer is refunded.

No third party holds the funds. The lock itself is the escrow.

Critical requirement: the preimage must be high entropy. P must be a full 256-bit random value, not a short PIN. If the release secret were a 6-digit code, the seller would know H and could brute-force the code instantly, then settle without doing the job. In practice the recipient shows or scans a QR code or long token, and not a number typed at the door.

Mode B: Coordinator-held release (hosted) - stretch goal

The service runs the node and holds P. It settles or cancels based on a verified condition (confirmation event, approval, or an operator decision), and then pays the seller by normal payment.

This is easier for users who have no node, but it makes the service a trusted custodian. If built, it must be labeled clearly as custodial in the UI and docs.

7. Core User Journeys
7.1 Create and lock
The seller creates an escrow offer: amount in sats, deadline, and a description.
The buyer opens the offer and accepts it. The buyer's client generates P and H, and keeps P.
The buyer's client sends H to the service.
The service asks the seller's node to create a hold invoice for H.
The buyer receives the invoice and pays it from their Lightning wallet or node.
When the node reports the invoice as accepted (funds held), the escrow becomes LOCKED. Both parties see it.
7.2 Release
The seller completes the job.
The buyer or recipient reveals P (by QR scan, copy-paste, or a link).
The seller's client submits P. The backend instructs the seller's node to settle the invoice.
The node reports the invoice as settled. The escrow becomes SETTLED.
7.3 Timeout and refund
The deadline passes with no release.
The backend cancels the hold invoice before the HTLC safety margin is reached.
The node reports the invoice as cancelled. The buyer's payment fails back and the buyer is refunded.
The escrow becomes REFUNDED.
7.4 Dispute (manual in MVP)

Either party can flag a dispute before the deadline. The MVP records the dispute and shows it to the operator, who can mark the escrow for cancellation. Disputes do not extend the HTLC beyond its safe window.

8. Functional Requirements

Priority: P0 = mandatory for the MVP, P1 = important but deferrable, P2 = future.

ID	Requirement	Priority
FR-01	Create an escrow with amount (sats), deadline, description, buyer and seller identities	P0
FR-02	Generate the release secret client-side with a cryptographically secure source; P never leaves the buyer or recipient except when released	P0
FR-03	Create a hold invoice on the seller's node for the supplied hash H	P0
FR-04	Subscribe to invoice state changes from the node and drive escrow state from them	P0
FR-05	Settle the invoice with P on release; verify sha256(P) == H before attempting settlement	P0
FR-06	Cancel the invoice on timeout, dispute resolution, or explicit cancellation, before the HTLC safety margin	P0
FR-07	Enforce valid state transitions in the backend; reject invalid ones	P0
FR-08	Idempotent operations: repeated create, settle, or cancel requests do not duplicate actions	P0
FR-09	Show lock status, countdown to deadline, release action, and final outcome in the web UI	P0
FR-10	Authentication and per-escrow authorization for buyer, seller, and operator	P0
FR-11	Audit log of every state change and the node event that caused it	P0
FR-12	Dispute flag and operator view	P1
FR-13	Delivery-style release via QR code scan	P1
FR-14	Mode B hosted/custodial escrow	P2
FR-15	Webhooks or live notifications; polling is acceptable for the MVP	P1
FR-16	Optional fiat display amount with a quote locked at creation (display only, no conversion)	P2

The frontend must never decide that a payment is locked or settled. Only the backend, based on node events, decides.

9. Escrow State Machine

States:

CREATED: terms agreed, no invoice yet.
INVOICE_ISSUED: hold invoice created, waiting for payment.
LOCKED: the node reports the invoice as accepted; funds are held.
SETTLED: the invoice settled; the seller was paid.
REFUNDED: the invoice was cancelled or expired; the buyer was refunded.
EXPIRED_UNPAID: the invoice expired before the buyer paid.
DISPUTED: flagged by a party while LOCKED.
FAILED: an unrecoverable error needing operator review.

Allowed transitions:

CREATED to INVOICE_ISSUED
INVOICE_ISSUED to LOCKED or EXPIRED_UNPAID
LOCKED to SETTLED, REFUNDED, or DISPUTED
DISPUTED to REFUNDED or SETTLED (operator decision, only within the safe window)
Any state to FAILED on unrecoverable error

SETTLED and REFUNDED are terminal.

The escrow state must always be reconcilable against the node's own invoice state. If they disagree, the node is authoritative.

10. Timeout and Safety Parameters

Timeouts are the hardest part of this product and need explicit, conservative defaults.

HTLC expiry is a hard limit. Each HTLC along the route expires at a block height. The seller's node must settle or cancel before that point with margin to spare, or it risks a force-closed channel. Holds are therefore measured in hours to roughly a day or two, not weeks.
Job deadline must fit inside the window. The escrow deadline must leave a buffer for the seller to settle after receiving P.
Settle-by deadline. The service enforces its own settle-by time well before the real HTLC expiry. LND also has a configurable hold-expiry margin that will cancel held HTLCs close to expiry; the service's deadline should be set earlier than that.
Invoice expiry. Unpaid invoices should expire quickly (for example, minutes), so a stale invoice cannot be paid late.
Final CLTV delta. Set the invoice's minimum final CLTV delta deliberately, since it sets how long the HTLC can stay held.
Validation at creation. Reject escrow deadlines longer than the configured maximum hold time.

Exact numbers must be tested on regtest or testnet and recorded in the docs.

11. Technical Architecture
11.1 Stack
Backend: Rust with Axum and Tokio.
Lightning node: LND (hold invoices via its invoices and router RPC services) in a regtest or testnet environment. Core Lightning is an alternative if its hold-invoice support is preferred.
Node integration: gRPC client against the node, with a dedicated adapter module.
Database: PostgreSQL with SQLx for escrows, events, and the audit log.
Frontend: Next.js, TypeScript, Tailwind, with Zod for form validation.
Local environment: Docker Compose, plus a regtest network (for example with Polar) with at least a buyer node, a seller node, and a routing node between them.
11.2 Logical modules
Web client: buyer and seller interfaces, QR display and scan.
API layer: authentication, validation, authorization.
Escrow core: state machine, deadlines, idempotency.
Lightning adapter: create hold invoice, subscribe to invoice updates, settle, cancel. Isolated behind an interface so it is testable with a mock.
Persistence: escrows, node events, audit log.
Scheduler: watches deadlines and triggers cancellation before the safety margin.
11.3 Architecture constraints
The node adapter must be a separate module behind a trait.
All node calls need timeouts and explicit failure handling.
Invoice event subscriptions must reconnect and resume after a restart, with startup reconciliation against the node.
State changes must be transactional.
Use restricted node credentials (for example, a macaroon limited to invoice operations) and keep them out of source control.
Core logic should be testable without a running server or a live node.
12. Data Model

Escrow: id, buyer_id, seller_id, amount_sats, description, payment_hash (H), invoice_reference, state, deadline_at, settle_by_at, created_at, updated_at.

Node event: id, escrow_id, node_invoice_state, raw_event_hash, received_at.

Audit entry: id, escrow_id, actor, action, from_state, to_state, reason, created_at.

User: id, role, auth_identifier, created_at.

The preimage P must never be stored in the database or written to logs. The service only needs it transiently at the moment of settlement. Full invoices and payment identifiers must not appear in ordinary logs or public responses.

13. API Surface (Proposed)
Method	Endpoint	Purpose
POST	/api/v1/escrows	Create an escrow offer
GET	/api/v1/escrows/:id	Get escrow details and state
POST	/api/v1/escrows/:id/accept	Buyer accepts and submits payment hash H
GET	/api/v1/escrows/:id/invoice	Get the hold invoice to pay
POST	/api/v1/escrows/:id/release	Submit P to settle
POST	/api/v1/escrows/:id/cancel	Request cancellation (where permitted)
POST	/api/v1/escrows/:id/dispute	Flag a dispute
GET	/api/v1/escrows/:id/events	Audit and event history
GET	/health	Health check, including node connectivity

The final contract must define authentication, schemas, error codes, and idempotency keys for state-changing calls.

14. Security, Privacy, and Trust
Honest trust model. In Mode A, no one holds the funds, but the seller's node (or the backend operating it) sees P at settlement time, and the coordinator is trusted to relay state correctly. State this in the product and docs.
High-entropy preimage. Enforce a 32-byte random P; never accept short codes as the release secret.
Verify before settling. Check that sha256(P) matches the stored H before calling settle.
Never trust the client for payment state. Payment status comes from node events only.
Credential hygiene. Restricted macaroons, secrets outside source control, TLS on all connections.
Logging. No preimages, macaroons, or full invoices in logs.
Rate limiting and validation on public endpoints.
Authorization on every action, scoped to the escrow's buyer, seller, or operator.
What this does not prove. Revealing P shows the buyer chose to release, not that the work was good. Escrow reduces the risk of non-payment and non-delivery, but does not eliminate fraud, quality disputes, or bad-faith buyers who refuse to release.
15. Non-Functional Requirements
Reliability: state changes are atomic; node outages must not corrupt escrow records; restart recovery reconciles against the node.
Safety under retries: repeated requests never double-settle or double-cancel.
Timing accuracy: cancellations must fire with enough margin before the HTLC safety window; monitor scheduler lag.
Observability: structured logs, correlation IDs, a health check that includes node status, and separate tracking of settle and cancel failures.
Performance targets (MVP, to be validated): ordinary API requests under 500 ms at p95 excluding node calls; settle and cancel calls with explicit timeouts.
16. MVP Acceptance Criteria
A seller can create an escrow and a buyer can accept it.
A real hold invoice is created on the seller's node for the buyer's hash.
The buyer pays and the escrow moves to LOCKED only after the node reports the invoice as accepted.
Supplying the correct P settles the invoice and moves the escrow to SETTLED.
Supplying an incorrect P is rejected and does not change state.
If the deadline passes with no release, the invoice is cancelled and the escrow moves to REFUNDED, and the buyer's funds return.
An invoice that is never paid moves to EXPIRED_UNPAID.
Duplicate release or cancel requests do not cause duplicate actions.
Restarting the backend mid-escrow does not lose or corrupt state; it reconciles with the node.
Users can only see and act on escrows they are permitted to.
P never appears in the database or logs.
Setup instructions and API documentation exist, and the main flow and key failure paths have automated tests.
17. Testing Strategy

Unit tests: state transitions, deadline and settle-by calculations, hash verification, authorization rules.

Integration tests: adapter against a regtest node: create hold invoice, pay, observe accepted state, settle, cancel, and expire; reconnect behavior of the invoice subscription.

End-to-end: two users and a three-node regtest network, running the full happy path and the full refund path.

Failure tests:

Wrong preimage submitted.
Release submitted twice.
Seller's node offline at settle time.
Backend restarts during a held payment.
Payment never made.
Deadline reached while a dispute is open.
Unauthorized user attempts to release or cancel.
18. Milestones

Scale phases to the time available. Build in this order, because the node integration is the riskiest part.

Phase 1: Prove the primitive (do this first). Stand up a regtest network. From a script or minimal Rust program, create a hold invoice for a known hash, pay it, observe the accepted state, then settle it. Repeat with a cancel. Do not start the UI until this works.

Phase 2: Escrow core. State machine, database schema, idempotency, node-event-driven transitions, and the deadline scheduler.

Phase 3: API. Endpoints, authentication, authorization, audit log, restart reconciliation.

Phase 4: Frontend. Buyer and seller views: lock status, countdown, QR release, outcome display.

Phase 5: Hardening and demo. Failure tests, timeout tuning on the target network, documentation, and a rehearsed demo.

19. Demo Script

Two outcomes make the demo land.

Happy path: seller creates an offer, buyer accepts and pays, the UI shows funds locked, the buyer shows the release QR, the seller scans it, and the payment settles with the seller's balance updating.
Refund path: run a second escrow with a short deadline, let the timer expire, and show the automatic refund with no human action.

The refund path is the proof that this is a real escrow and not just a payment with extra steps.

20. Risks and Mitigations
Risk	Mitigation
Node integration takes longer than expected	Prove the hold-invoice cycle in a script first; cut UI scope before cutting node work
Short release code is brute-forceable	Require a 256-bit preimage; use QR or long token for release
HTLC expires before the seller settles, risking a channel close	Conservative settle-by deadline, scheduler monitoring, short maximum hold
Seller's node offline at settle time, so the buyer is refunded despite completed work	Health monitoring, margin before expiry, clear communication of the risk
Buyer withholds P after receiving goods	Real-world proof (the recipient holds P and is needed at handoff), dispute flag, honest documentation of the limit
Held payments tie up routing liquidity	Short holds, small amounts, document the tradeoff
Payers' wallets time out or mishandle long-pending payments	Test with at least one real wallet and your own node; document supported payers
Sellers without hold-invoice-capable nodes cannot participate	State the limit; offer Mode B hosted node as a stretch goal
Project appears insufficiently differentiated	Lead with the refund demo and a specific use case (delivery or freelance)
21. Open Questions
Which hackathon track and judging criteria apply, and is Lightning mandatory?
Which use case leads the demo: courier delivery, freelance work, or P2P trade?
LND or Core Lightning for the node adapter?
Does the demo target regtest only, or testnet or signet with real wallets?
Is Mode B (hosted) in scope for the submission, or only documented as future work?
What is the maximum supported hold time given the target network and node settings?
22. Future Roadmap
v0.2: Hosted mode for users without nodes, with clearly stated custody terms.
v0.3: Arbitration: a designated third party who can sign a release or refund decision.
v0.4: Milestone-based escrows (multiple smaller holds) for longer freelance jobs.
v1.0: Integration with delivery and marketplace platforms through an API, and investigation of stronger conditional-payment mechanisms for longer holds.