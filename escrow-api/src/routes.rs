//! Endpoint handlers.

use axum::{
    extract::{FromRequest, Path, Request, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use escrow_core::{
    adapter::LightningAdapter,
    model::{Escrow, PaymentHash, Preimage, Role, User},
    service::NewEscrow,
    state::EscrowState,
    store::{Store, UserStore},
};
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::{
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
    A: LightningAdapter + 'static,
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
    A: LightningAdapter + 'static,
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
    A: LightningAdapter + 'static,
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
    A: LightningAdapter + 'static,
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

    // Idempotency-Key: a repeated create returns the first result instead of a second escrow.
    let key = headers.get("idempotency-key").and_then(|v| v.to_str().ok()).map(str::to_string);
    let body_hash = hex::encode(Sha256::digest(
        format!("{}|{}|{}|{}", buyer_id.as_deref().unwrap_or(""), req.amount_sats, req.description, req.hold_secs).as_bytes(),
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

    let now = (st.now)();
    let escrow = st
        .service
        .create(
            NewEscrow {
                buyer_id,
                seller_id: user.id.clone(),
                amount_sats: req.amount_sats,
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
    A: LightningAdapter + 'static,
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
    A: LightningAdapter + 'static,
    S: Store + UserStore + Clone + 'static,
{
    let (escrow, party) = load(&st, &id, &user).await?;
    Ok(Json(escrow_view(&escrow, party)))
}

#[derive(Deserialize)]
pub struct AcceptReq {
    payment_hash: String,
}

pub async fn accept<A, S>(
    State(st): State<AppState<A, S>>,
    Auth(user): Auth,
    Path(id): Path<String>,
    ApiJson(req): ApiJson<AcceptReq>,
) -> Result<Json<Value>, ApiError>
where
    A: LightningAdapter + 'static,
    S: Store + UserStore + Clone + 'static,
{
    let (_, party) = load(&st, &id, &user).await?;
    if party != Party::Buyer {
        return Err(ApiError::forbidden("only the buyer can accept"));
    }
    let hash: PaymentHash = parse_hex32("payment_hash", &req.payment_hash)?;
    let escrow = st.service.accept(&id, hash, &user.id, (st.now)()).await?;
    Ok(Json(escrow_view(&escrow, party)))
}

pub async fn invoice<A, S>(
    State(st): State<AppState<A, S>>,
    Auth(user): Auth,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError>
where
    A: LightningAdapter + 'static,
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
    A: LightningAdapter + 'static,
    S: Store + UserStore + Clone + 'static,
{
    let (_, party) = load(&st, &id, &user).await?;
    if party == Party::Operator {
        return Err(ApiError::forbidden("only the buyer or seller can release"));
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
    A: LightningAdapter + 'static,
    S: Store + UserStore + Clone + 'static,
{
    let (escrow, party) = load(&st, &id, &user).await?;
    // Cancelling after the buyer has paid gives the buyer a refund, so the buyer may not do it
    // alone: that would strip the seller's protection. Sellers and operators may.
    if party == Party::Buyer && !matches!(escrow.state, EscrowState::Created | EscrowState::InvoiceIssued) {
        return Err(ApiError::forbidden("the buyer can only cancel before paying"));
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
    A: LightningAdapter + 'static,
    S: Store + UserStore + Clone + 'static,
{
    let (_, party) = load(&st, &id, &user).await?;
    if party == Party::Operator {
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
    A: LightningAdapter + 'static,
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
    A: LightningAdapter + 'static,
    S: Store + UserStore + Clone + 'static,
{
    if user.role != Role::Operator {
        return Err(ApiError::forbidden("operator only"));
    }
    let list: Vec<Value> =
        st.service.list_active().await?.iter().map(|e| escrow_view(e, Party::Operator)).collect();
    Ok(Json(json!({ "escrows": list })))
}
