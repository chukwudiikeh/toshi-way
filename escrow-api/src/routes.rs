//! Endpoint handlers.

use axum::{
    extract::{FromRequest, Path, Request, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use escrow_core::{
    adapter::{NodeConnectError, NodeCredentials, NodeDirectory, NodeSummary},
    fiat::{self, FiatQuote},
    model::{Escrow, PaymentHash, Preimage, Role, User},
    service::{Decision, NewEscrow},
    state::EscrowState,
    store::{Store, UserStore},
};
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::{
    arbiter,
    rates::RateError,
    auth::{hash_token, is_prospective_buyer, new_token, new_user_id, party_of, Auth, Party},
    error::ApiError,
    AppState,
};

/// `Json` with errors in the API's own format.
pub struct ApiJson<T>(pub T);

impl<T, St> FromRequest<St> for ApiJson<T>
where
    T: DeserializeOwned,
    St: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &St) -> Result<Self, ApiError> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(v)) => Ok(ApiJson(v)),
            Err(rej) => {
                let status = rej.status();
                let code = if status == StatusCode::PAYLOAD_TOO_LARGE { "payload_too_large" } else { "invalid_body" };
                Err(ApiError::new(status, code, rej.body_text()))
            }
        }
    }
}

fn escrow_view(e: &Escrow, party: Party) -> Value {
    json!({
        "id": e.id,
        "buyer_id": e.buyer_id,
        "seller_id": e.seller_id,
        "amount_sats": e.amount_sats,
        "description": e.description,
        "payment_hash": e.payment_hash.map(hex::encode),
        "state": e.state.as_str(),
        "invoice_expires_at": e.invoice_expires_at,
        "deadline_at": e.deadline_at,
        "created_at": e.created_at,
        "updated_at": e.updated_at,
        "your_role": party.as_str(),
        // Whose node holds the invoice: "own" pays the seller directly; "shared" is the service's
        // node, so the service receives the sats and owes them to the seller (custodial).
        "seller_node": e.node.as_str(),
        // Arbitration. The key is public; the sealed secret is never in this view (only the arbiter
        // can fetch it, from /arbitration), just whether one exists.
        "arbiter_id": e.arbiter_id,
        "arbiter_key": e.arbiter_key,
        "arbiter_fingerprint": e.arbiter_key.as_deref().and_then(arbiter::fingerprint),
        "arbiter_sealed": e.arbiter_sealed.is_some(),
        "arbiter_confirmed_at": e.arbiter_confirmed_at,
        // What the sum was worth when the offer was made, for display only. Locked at creation.
        "fiat": e.fiat.as_ref().map(|f| json!({
            "currency": f.currency,
            "exponent": fiat::exponent(&f.currency),
            "amount_minor": f.amount_minor,
            "rate_minor_per_btc": f.rate_minor_per_btc,
            "quoted_at": f.quoted_at,
            "source": f.source,
        })),
    })
}

fn parse_hex32(field: &str, s: &str) -> Result<[u8; 32], ApiError> {
    hex::decode(s)
        .ok()
        .and_then(|b| <[u8; 32]>::try_from(b).ok())
        .ok_or_else(|| ApiError::bad_request("invalid_hex", format!("{field} must be exactly 64 hex characters")))
}

/// Load an escrow the caller may see. Callers with no standing get 404, not 403.
async fn load<A, S>(st: &AppState<A, S>, id: &str, user: &User) -> Result<(Escrow, Party), ApiError>
where
    A: NodeDirectory + 'static,
    S: Store + UserStore + Clone + 'static,
{
    let escrow = st.service.get(id).await?;
    let party = party_of(&escrow, user)
        .or_else(|| is_prospective_buyer(&escrow, user).then_some(Party::Buyer))
        .ok_or_else(ApiError::not_found)?;
    Ok((escrow, party))
}

pub async fn health<A, S>(State(st): State<AppState<A, S>>) -> Response
where
    A: NodeDirectory + 'static,
    S: Store + UserStore + Clone + 'static,
{
    // `server_time` lets browsers correct countdowns for their own clock skew.
    let server_time = (st.now)();
    match st.service.node_health().await {
        Ok(()) => Json(json!({ "status": "ok", "node": "up", "server_time": server_time })).into_response(),
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "status": "degraded", "node": "down", "server_time": server_time })),
        )
            .into_response(),
    }
}

/// Who the caller is, so a client holding only a token can learn its id and role.
pub async fn me(Auth(user): Auth) -> Json<Value> {
    Json(json!({ "id": user.id, "role": user.role.as_str() }))
}

pub async fn register<A, S>(State(st): State<AppState<A, S>>) -> Result<(StatusCode, Json<Value>), ApiError>
where
    A: NodeDirectory + 'static,
    S: Store + UserStore + Clone + 'static,
{
    let token = new_token();
    let user = User { id: new_user_id(), role: Role::User, created_at: (st.now)() };
    st.users.insert_user(user.clone(), hash_token(&token)).await?;
    // The token is shown once and cannot be recovered: only its hash is stored.
    Ok((StatusCode::CREATED, Json(json!({ "id": user.id, "role": user.role.as_str(), "token": token }))))
}

#[derive(Deserialize)]
pub struct CreateReq {
    /// Omit (or leave empty) for an open offer: the first user to accept it becomes the buyer.
    #[serde(default)]
    buyer_id: Option<String>,
    /// A trusted third party who decides if there is a dispute. They must have registered a key.
    #[serde(default)]
    arbiter_id: Option<String>,
    /// Show the amount in this currency too (display only; the escrow is still in sats). The rate
    /// is taken now and locked.
    #[serde(default)]
    fiat_currency: Option<String>,
    amount_sats: u64,
    description: String,
    /// Seconds from now until the settle-by deadline.
    hold_secs: i64,
}

pub async fn create_escrow<A, S>(
    State(st): State<AppState<A, S>>,
    Auth(user): Auth,
    headers: HeaderMap,
    ApiJson(req): ApiJson<CreateReq>,
) -> Result<(StatusCode, Json<Value>), ApiError>
where
    A: NodeDirectory + 'static,
    S: Store + UserStore + Clone + 'static,
{
    if user.role == Role::Operator {
        return Err(ApiError::forbidden("operators cannot be a party to an escrow"));
    }
    if req.amount_sats == 0 || req.amount_sats > st.config.max_amount_sats {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_amount",
            format!("amount must be between 1 and {} sats", st.config.max_amount_sats),
        ));
    }
    if req.description.chars().count() > st.config.max_description_len {
        return Err(ApiError::bad_request(
            "invalid_description",
            format!("description is limited to {} characters", st.config.max_description_len),
        ));
    }
    let buyer_id = req.buyer_id.as_deref().map(str::trim).filter(|b| !b.is_empty()).map(str::to_string);
    if let Some(buyer_id) = &buyer_id {
        match st.users.get_user(buyer_id).await? {
            Some(b) if b.role == Role::User => {}
            _ => return Err(ApiError::bad_request("unknown_buyer", "buyer_id does not match a user")),
        }
    }

    let arbiter_id = req.arbiter_id.as_deref().map(str::trim).filter(|a| !a.is_empty()).map(str::to_string);
    let arbiter = match &arbiter_id {
        None => None,
        Some(id) => {
            match st.users.get_user(id).await? {
                Some(a) if a.role == Role::User => {}
                _ => return Err(ApiError::bad_request("unknown_arbiter", "arbiter_id does not match a user")),
            }
            if *id == user.id || buyer_id.as_deref() == Some(id.as_str()) {
                return Err(ApiError::bad_request(
                    "invalid_arbiter",
                    "the arbiter must be someone other than the buyer and the seller",
                ));
            }
            let key = st.users.arbiter_key(id).await?.ok_or_else(|| {
                ApiError::bad_request("arbiter_has_no_key", "that user has not set up a key to act as an arbiter")
            })?;
            // Recorded on the escrow now, so the key buyers encrypt to cannot be swapped later.
            Some((id.clone(), key))
        }
    };

    // A display-only fiat value. If the user asked for one and we cannot give it, say so rather than
    // quietly dropping it; it never affects the escrow itself.
    let now = (st.now)();
    let fiat_currency = req.fiat_currency.as_deref().map(str::trim).filter(|c| !c.is_empty());
    let fiat = match fiat_currency {
        None => None,
        Some(code) => {
            let code = fiat::normalize(code)
                .ok_or_else(|| ApiError::bad_request("unsupported_currency", format!("'{code}' is not a supported currency")))?;
            let rate = st.rates.rate(&code, now).await.map_err(rate_error)?;
            let quote = FiatQuote::new(req.amount_sats, &code, rate.rate_minor_per_btc, rate.as_of, &rate.source)
                .map_err(|e| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "rate_unavailable", e.to_string()))?;
            Some(quote)
        }
    };

    // Idempotency-Key: a repeated create returns the first result instead of a second escrow.
    let key = headers.get("idempotency-key").and_then(|v| v.to_str().ok()).map(str::to_string);
    let body_hash = hex::encode(Sha256::digest(
        format!(
            "{}|{}|{}|{}|{}|{}",
            buyer_id.as_deref().unwrap_or(""),
            arbiter_id.as_deref().unwrap_or(""),
            fiat.as_ref().map(|f| f.currency.as_str()).unwrap_or(""),
            req.amount_sats,
            req.description,
            req.hold_secs
        )
        .as_bytes(),
    ));
    let mut cache = match &key {
        Some(_) => Some(st.idempotency.entries.lock().await),
        None => None,
    };
    if let (Some(cache), Some(key)) = (&cache, &key) {
        if let Some((seen_hash, response)) = cache.get(&(user.id.clone(), key.clone())) {
            if *seen_hash != body_hash {
                return Err(ApiError::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "idempotency_key_reuse",
                    "this Idempotency-Key was already used with a different request",
                ));
            }
            return Ok((StatusCode::CREATED, Json(response.clone())));
        }
    }

    let escrow = st
        .service
        .create(
            NewEscrow {
                buyer_id,
                seller_id: user.id.clone(),
                arbiter,
                amount_sats: req.amount_sats,
                fiat,
                description: req.description,
                deadline_at: now.saturating_add(req.hold_secs),
            },
            now,
        )
        .await?;
    let view = escrow_view(&escrow, Party::Seller);
    if let (Some(cache), Some(key)) = (cache.as_mut(), key) {
        cache.insert((user.id, key), (body_hash, view.clone()));
    }
    Ok((StatusCode::CREATED, Json(view)))
}

/// The caller's own escrows, as buyer or seller.
pub async fn list_mine<A, S>(
    State(st): State<AppState<A, S>>,
    Auth(user): Auth,
) -> Result<Json<Value>, ApiError>
where
    A: NodeDirectory + 'static,
    S: Store + UserStore + Clone + 'static,
{
    let list: Vec<Value> = st
        .service
        .list_for_user(&user.id)
        .await?
        .iter()
        .filter_map(|e| party_of(e, &user).map(|p| escrow_view(e, p)))
        .collect();
    Ok(Json(json!({ "escrows": list })))
}

pub async fn get_escrow<A, S>(
    State(st): State<AppState<A, S>>,
    Auth(user): Auth,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError>
where
    A: NodeDirectory + 'static,
    S: Store + UserStore + Clone + 'static,
{
    let (escrow, party) = load(&st, &id, &user).await?;
    Ok(Json(escrow_view(&escrow, party)))
}

#[derive(Deserialize)]
pub struct AcceptReq {
    payment_hash: String,
    /// The release secret encrypted to the arbiter's key, required when the escrow has an arbiter.
    #[serde(default)]
    sealed_secret: Option<String>,
}

pub async fn accept<A, S>(
    State(st): State<AppState<A, S>>,
    Auth(user): Auth,
    Path(id): Path<String>,
    ApiJson(req): ApiJson<AcceptReq>,
) -> Result<Json<Value>, ApiError>
where
    A: NodeDirectory + 'static,
    S: Store + UserStore + Clone + 'static,
{
    let (_, party) = load(&st, &id, &user).await?;
    if party != Party::Buyer {
        return Err(ApiError::forbidden("only the buyer can accept"));
    }
    let hash: PaymentHash = parse_hex32("payment_hash", &req.payment_hash)?;
    // The invoice is made on the seller's node; keep that node from being swapped meanwhile.
    let _node_guard = st.node_changes.read().await;
    let escrow = st.service.accept_with(&id, hash, &user.id, (st.now)(), req.sealed_secret).await?;
    Ok(Json(escrow_view(&escrow, party)))
}

pub async fn invoice<A, S>(
    State(st): State<AppState<A, S>>,
    Auth(user): Auth,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError>
where
    A: NodeDirectory + 'static,
    S: Store + UserStore + Clone + 'static,
{
    let (escrow, party) = load(&st, &id, &user).await?;
    if party != Party::Buyer {
        return Err(ApiError::forbidden("only the buyer can fetch the invoice"));
    }
    match (escrow.state, escrow.invoice) {
        (EscrowState::InvoiceIssued, Some(payment_request)) => {
            Ok(Json(json!({ "payment_request": payment_request, "expires_at": escrow.invoice_expires_at })))
        }
        (state, _) => Err(ApiError::new(
            StatusCode::CONFLICT,
            "invalid_state",
            format!("there is no payable invoice while the escrow is {state}"),
        )),
    }
}

#[derive(Deserialize)]
pub struct ReleaseReq {
    preimage: String,
}

pub async fn release<A, S>(
    State(st): State<AppState<A, S>>,
    Auth(user): Auth,
    Path(id): Path<String>,
    ApiJson(req): ApiJson<ReleaseReq>,
) -> Result<Json<Value>, ApiError>
where
    A: NodeDirectory + 'static,
    S: Store + UserStore + Clone + 'static,
{
    let (_, party) = load(&st, &id, &user).await?;
    if matches!(party, Party::Operator | Party::Arbiter) {
        return Err(ApiError::forbidden("only the buyer or seller can release; an arbiter decides disputes"));
    }
    // Only a full 256-bit secret is accepted: short codes can be brute-forced from the hash.
    let preimage = Preimage::from_hex(&req.preimage).ok_or_else(|| {
        ApiError::bad_request("invalid_preimage", "preimage must be exactly 64 hex characters (32 bytes)")
    })?;
    let escrow = st.service.release(&id, &preimage, &user.id, (st.now)()).await?;
    Ok(Json(escrow_view(&escrow, party)))
}

pub async fn cancel<A, S>(
    State(st): State<AppState<A, S>>,
    Auth(user): Auth,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError>
where
    A: NodeDirectory + 'static,
    S: Store + UserStore + Clone + 'static,
{
    let (escrow, party) = load(&st, &id, &user).await?;
    // Cancelling after the buyer has paid gives the buyer a refund, so the buyer may not do it
    // alone: that would strip the seller's protection. Sellers and operators may.
    if party == Party::Buyer && !matches!(escrow.state, EscrowState::Created | EscrowState::InvoiceIssued) {
        return Err(ApiError::forbidden("the buyer can only cancel before paying"));
    }
    // The arbiter acts only through their decision, and nobody can overrule one: once an arbitrated
    // escrow is disputed the operator can no longer cancel it either.
    if party == Party::Arbiter {
        return Err(ApiError::forbidden("an arbiter decides a dispute; they cannot cancel an escrow"));
    }
    if party == Party::Operator && escrow.arbiter_id.is_some() && escrow.state == EscrowState::Disputed {
        return Err(ApiError::forbidden("this dispute is being decided by the arbiter"));
    }
    // Someone who has only seen an open offer's link is not its buyer and cannot decline it.
    if party == Party::Buyer && escrow.buyer_id.as_deref() != Some(user.id.as_str()) {
        return Err(ApiError::forbidden("only the buyer can decline an offer"));
    }
    let reason = format!("cancelled by {}", party.as_str());
    let escrow = st.service.cancel(&id, &user.id, &reason, (st.now)()).await?;
    Ok(Json(escrow_view(&escrow, party)))
}

#[derive(Deserialize)]
pub struct DisputeReq {
    reason: String,
}

pub async fn dispute<A, S>(
    State(st): State<AppState<A, S>>,
    Auth(user): Auth,
    Path(id): Path<String>,
    ApiJson(req): ApiJson<DisputeReq>,
) -> Result<Json<Value>, ApiError>
where
    A: NodeDirectory + 'static,
    S: Store + UserStore + Clone + 'static,
{
    let (_, party) = load(&st, &id, &user).await?;
    if matches!(party, Party::Operator | Party::Arbiter) {
        return Err(ApiError::forbidden("only the buyer or seller can open a dispute"));
    }
    if req.reason.trim().is_empty() || req.reason.chars().count() > st.config.max_description_len {
        return Err(ApiError::bad_request("invalid_reason", "reason must be 1 to 500 characters"));
    }
    let escrow = st.service.dispute(&id, &user.id, &req.reason, (st.now)()).await?;
    Ok(Json(escrow_view(&escrow, party)))
}

pub async fn events<A, S>(
    State(st): State<AppState<A, S>>,
    Auth(user): Auth,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError>
where
    A: NodeDirectory + 'static,
    S: Store + UserStore + Clone + 'static,
{
    load(&st, &id, &user).await?;
    let audit: Vec<Value> = st
        .service
        .audit_log(&id)
        .await?
        .into_iter()
        .map(|a| {
            json!({
                "actor": a.actor, "action": a.action, "from_state": a.from_state.as_str(),
                "to_state": a.to_state.as_str(), "reason": a.reason, "at": a.created_at,
            })
        })
        .collect();
    let node: Vec<Value> = st
        .service
        .node_events(&id)
        .await?
        .into_iter()
        .map(|n| json!({ "node_invoice_state": n.node_invoice_state.as_str(), "at": n.received_at }))
        .collect();
    Ok(Json(json!({ "audit": audit, "node_events": node })))
}

pub async fn operator_list<A, S>(
    State(st): State<AppState<A, S>>,
    Auth(user): Auth,
) -> Result<Json<Value>, ApiError>
where
    A: NodeDirectory + 'static,
    S: Store + UserStore + Clone + 'static,
{
    if user.role != Role::Operator {
        return Err(ApiError::forbidden("operator only"));
    }
    let list: Vec<Value> =
        st.service.list_active().await?.iter().map(|e| escrow_view(e, Party::Operator)).collect();
    Ok(Json(json!({ "escrows": list })))
}

// --- The seller's own Lightning node --------------------------------------------------------

/// How the caller's escrows will be served. Never includes the macaroon or certificate.
async fn node_view<A, S>(st: &AppState<A, S>, user: &User) -> Result<Value, ApiError>
where
    A: NodeDirectory + 'static,
    S: Store + UserStore + Clone + 'static,
{
    let dir = st.service.directory();
    let own: Option<NodeSummary> = dir.seller_node_summary(&user.id).await?;
    let shared = dir.offers_shared_node();
    let mode = match (&own, shared) {
        (Some(_), _) => "own",
        (None, true) => "shared",
        (None, false) => "none",
    };
    Ok(json!({
        "mode": mode,
        "address": own.as_ref().map(|n| n.address.clone()),
        "connected_at": own.as_ref().map(|n| n.updated_at),
        // True when sellers without a node still get a (custodial) shared one.
        "shared_available": shared,
        // False when the server has no credential key, so sellers cannot connect nodes at all.
        "own_nodes_enabled": dir.accepts_seller_nodes(),
        "can_change": !st.service.has_active_own_node_escrows(&user.id).await?,
    }))
}

fn require_user(user: &User) -> Result<(), ApiError> {
    if user.role == Role::Operator {
        return Err(ApiError::forbidden("operators cannot have a seller node"));
    }
    Ok(())
}

pub async fn node_get<A, S>(State(st): State<AppState<A, S>>, Auth(user): Auth) -> Result<Json<Value>, ApiError>
where
    A: NodeDirectory + 'static,
    S: Store + UserStore + Clone + 'static,
{
    require_user(&user)?;
    Ok(Json(node_view(&st, &user).await?))
}

#[derive(Deserialize)]
pub struct NodeReq {
    address: String,
    tls_cert: String,
    /// Hex of an invoice-only macaroon.
    macaroon: String,
}

pub async fn node_put<A, S>(
    State(st): State<AppState<A, S>>,
    Auth(user): Auth,
    ApiJson(req): ApiJson<NodeReq>,
) -> Result<Json<Value>, ApiError>
where
    A: NodeDirectory + 'static,
    S: Store + UserStore + Clone + 'static,
{
    require_user(&user)?;
    // Exclusive: no invoice may be created on the old node while it is being replaced.
    let _guard = st.node_changes.write().await;
    if st.service.has_active_own_node_escrows(&user.id).await? {
        return Err(node_in_use());
    }
    let creds = NodeCredentials { address: req.address, tls_cert: req.tls_cert, macaroon_hex: req.macaroon };
    match st.service.directory().register_seller_node(&user.id, creds, (st.now)()).await {
        Ok(_) => {}
        Err(NodeConnectError::Invalid(m)) => return Err(ApiError::bad_request("invalid_node", m)),
        Err(NodeConnectError::CheckFailed(m)) => {
            return Err(ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "node_check_failed", m))
        }
        Err(NodeConnectError::Adapter(e)) => return Err(escrow_core::service::ServiceError::from(e).into()),
    }
    Ok(Json(node_view(&st, &user).await?))
}

pub async fn node_delete<A, S>(State(st): State<AppState<A, S>>, Auth(user): Auth) -> Result<Json<Value>, ApiError>
where
    A: NodeDirectory + 'static,
    S: Store + UserStore + Clone + 'static,
{
    require_user(&user)?;
    let _guard = st.node_changes.write().await;
    if st.service.has_active_own_node_escrows(&user.id).await? {
        return Err(node_in_use());
    }
    st.service.directory().remove_seller_node(&user.id).await?;
    Ok(Json(node_view(&st, &user).await?))
}

fn node_in_use() -> ApiError {
    ApiError::new(
        StatusCode::CONFLICT,
        "node_in_use",
        "you have escrows in progress on this node; finish or cancel them before changing it",
    )
}

// --- Arbitration ----------------------------------------------------------------------------

/// An arbiter registers the public key people encrypt release secrets to.
#[derive(Deserialize)]
pub struct ArbiterKeyReq {
    public_key: String,
}

pub async fn arbiter_key_put<A, S>(
    State(st): State<AppState<A, S>>,
    Auth(user): Auth,
    ApiJson(req): ApiJson<ArbiterKeyReq>,
) -> Result<Json<Value>, ApiError>
where
    A: NodeDirectory + 'static,
    S: Store + UserStore + Clone + 'static,
{
    if user.role == Role::Operator {
        return Err(ApiError::forbidden("operators cannot be arbiters"));
    }
    arbiter::parse_public_key(&req.public_key).map_err(|m| ApiError::bad_request("invalid_key", m))?;
    let key = req.public_key.trim();
    st.users.put_arbiter_key(&user.id, key, (st.now)()).await?;
    Ok(Json(json!({ "public_key": key, "fingerprint": arbiter::fingerprint(key) })))
}

pub async fn arbiter_key_get<A, S>(State(st): State<AppState<A, S>>, Auth(user): Auth) -> Result<Json<Value>, ApiError>
where
    A: NodeDirectory + 'static,
    S: Store + UserStore + Clone + 'static,
{
    let key = st.users.arbiter_key(&user.id).await?;
    Ok(Json(json!({ "public_key": key, "fingerprint": key.as_deref().and_then(arbiter::fingerprint) })))
}

/// Load an escrow the caller is the arbiter of; everyone else gets the usual 404 or a 403.
async fn load_as_arbiter<A, S>(st: &AppState<A, S>, id: &str, user: &User) -> Result<Escrow, ApiError>
where
    A: NodeDirectory + 'static,
    S: Store + UserStore + Clone + 'static,
{
    let (escrow, party) = load(st, id, user).await?;
    if party != Party::Arbiter {
        return Err(ApiError::forbidden("only the arbiter can do this"));
    }
    Ok(escrow)
}

/// What the arbiter needs to judge: the buyer's secret sealed to them, and the hash it must match.
/// Available from when the buyer accepts until the escrow ends.
pub async fn arbitration_get<A, S>(
    State(st): State<AppState<A, S>>,
    Auth(user): Auth,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError>
where
    A: NodeDirectory + 'static,
    S: Store + UserStore + Clone + 'static,
{
    let e = load_as_arbiter(&st, &id, &user).await?;
    match (e.state, e.arbiter_sealed.as_ref()) {
        (EscrowState::InvoiceIssued | EscrowState::Locked | EscrowState::Disputed, Some(sealed)) => Ok(Json(json!({
            "sealed_secret": sealed,
            "payment_hash": e.payment_hash.map(hex::encode),
            "arbiter_key": e.arbiter_key,
            "state": e.state.as_str(),
        }))),
        (state, _) => Err(ApiError::new(
            StatusCode::CONFLICT,
            "invalid_state",
            format!("there is nothing for an arbiter to judge while the escrow is {state}"),
        )),
    }
}

pub async fn arbitration_confirm<A, S>(
    State(st): State<AppState<A, S>>,
    Auth(user): Auth,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError>
where
    A: NodeDirectory + 'static,
    S: Store + UserStore + Clone + 'static,
{
    load_as_arbiter(&st, &id, &user).await?;
    let escrow = st.service.confirm_arbiter(&id, &user.id, (st.now)()).await?;
    Ok(Json(escrow_view(&escrow, Party::Arbiter)))
}

#[derive(Deserialize)]
pub struct DecideReq {
    /// "release" (pay the seller) or "refund" (return the buyer's money).
    decision: String,
    /// The buyer's secret, decrypted by the arbiter. Required for "release".
    #[serde(default)]
    preimage: Option<String>,
    #[serde(default)]
    note: Option<String>,
}

pub async fn arbitration_decide<A, S>(
    State(st): State<AppState<A, S>>,
    Auth(user): Auth,
    Path(id): Path<String>,
    ApiJson(req): ApiJson<DecideReq>,
) -> Result<Json<Value>, ApiError>
where
    A: NodeDirectory + 'static,
    S: Store + UserStore + Clone + 'static,
{
    load_as_arbiter(&st, &id, &user).await?;
    let note = req.note.unwrap_or_default();
    if note.chars().count() > st.config.max_description_len {
        return Err(ApiError::bad_request("invalid_reason", "the note is limited to 500 characters"));
    }
    let decision = match req.decision.as_str() {
        "refund" => Decision::Refund,
        "release" => {
            let hex = req.preimage.as_deref().unwrap_or("");
            // Same rule as any release: only a full 256-bit secret.
            Decision::Release(Preimage::from_hex(hex).ok_or_else(|| {
                ApiError::bad_request("invalid_preimage", "preimage must be exactly 64 hex characters (32 bytes)")
            })?)
        }
        _ => return Err(ApiError::bad_request("invalid_decision", "decision must be \"release\" or \"refund\"")),
    };
    let escrow = st.service.arbitrate(&id, &user.id, decision, &note, (st.now)()).await?;
    Ok(Json(escrow_view(&escrow, Party::Arbiter)))
}

// --- Fiat display rates -----------------------------------------------------------------------

fn rate_error(e: RateError) -> ApiError {
    match e {
        RateError::Disabled => ApiError::bad_request("fiat_disabled", "this server does not show fiat values"),
        RateError::Unsupported(c) => {
            ApiError::bad_request("unsupported_currency", format!("'{c}' is not offered by this server's price feed"))
        }
        RateError::Unavailable => ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "rate_unavailable",
            "the price feed is unavailable right now; try again, or leave the fiat value out",
        ),
    }
}

/// Whether fiat values are on, where the price comes from, and which currencies are offered.
pub async fn rates_list<A, S>(State(st): State<AppState<A, S>>, Auth(_user): Auth) -> Json<Value>
where
    A: NodeDirectory + 'static,
    S: Store + UserStore + Clone + 'static,
{
    let currencies: Vec<Value> = st
        .rates
        .currencies()
        .into_iter()
        .filter_map(|c| fiat::exponent(&c).map(|e| json!({ "code": c, "exponent": e })))
        .collect();
    Json(json!({ "enabled": !currencies.is_empty(), "source": st.rates.name(), "currencies": currencies }))
}

/// The current price of one bitcoin in a currency, so the form can preview a value before creating.
pub async fn rates_one<A, S>(
    State(st): State<AppState<A, S>>,
    Auth(_user): Auth,
    Path(currency): Path<String>,
) -> Result<Json<Value>, ApiError>
where
    A: NodeDirectory + 'static,
    S: Store + UserStore + Clone + 'static,
{
    let code = fiat::normalize(&currency)
        .ok_or_else(|| ApiError::bad_request("unsupported_currency", format!("'{currency}' is not a supported currency")))?;
    let rate = st.rates.rate(&code, (st.now)()).await.map_err(rate_error)?;
    Ok(Json(json!({
        "currency": code,
        "exponent": fiat::exponent(&code),
        "rate_minor_per_btc": rate.rate_minor_per_btc,
        "quoted_at": rate.as_of,
        "source": rate.source,
    })))
}
