//! Bearer-token authentication and per-escrow authorization.
//!
//! Tokens are random 256-bit values shown once at registration; only their SHA-256 is stored.

use axum::{extract::FromRequestParts, http::request::Parts};
use escrow_core::{
    adapter::LightningAdapter,
    model::{Escrow, Role, User},
    store::{Store, UserStore},
};
use rand::RngCore;
use sha2::{Digest, Sha256};

use crate::{error::ApiError, AppState};

pub fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

pub fn new_token() -> String {
    let mut b = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut b);
    format!("tw_{}", hex::encode(b))
}

pub fn new_user_id() -> String {
    let mut b = [0u8; 8];
    rand::rngs::OsRng.fill_bytes(&mut b);
    format!("u_{}", hex::encode(b))
}

/// The caller's relationship to one escrow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Party {
    Buyer,
    Seller,
    Operator,
}

impl Party {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Buyer => "buyer",
            Self::Seller => "seller",
            Self::Operator => "operator",
        }
    }
}

/// `None` means the user has no standing on this escrow and must not learn that it exists.
pub fn party_of(escrow: &Escrow, user: &User) -> Option<Party> {
    if user.id == escrow.buyer_id {
        Some(Party::Buyer)
    } else if user.id == escrow.seller_id {
        Some(Party::Seller)
    } else if user.role == Role::Operator {
        Some(Party::Operator)
    } else {
        None
    }
}

/// The authenticated caller.
pub struct Auth(pub User);

impl<A, S> FromRequestParts<AppState<A, S>> for Auth
where
    A: LightningAdapter + 'static,
    S: Store + UserStore + Clone + 'static,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState<A, S>) -> Result<Self, ApiError> {
        let token = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .ok_or_else(ApiError::unauthorized)?;
        let user = state
            .users
            .user_by_token_hash(&hash_token(token))
            .await?
            .ok_or_else(ApiError::unauthorized)?;
        Ok(Auth(user))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use escrow_core::state::EscrowState;

    fn escrow() -> Escrow {
        Escrow {
            id: "e".into(),
            buyer_id: "b".into(),
            seller_id: "s".into(),
            amount_sats: 1,
            description: String::new(),
            payment_hash: None,
            invoice: None,
            invoice_expires_at: None,
            state: EscrowState::Created,
            deadline_at: 0,
            created_at: 0,
            updated_at: 0,
        }
    }

    fn user(id: &str, role: Role) -> User {
        User { id: id.into(), role, created_at: 0 }
    }

    #[test]
    fn parties_are_resolved_per_escrow() {
        let e = escrow();
        assert_eq!(party_of(&e, &user("b", Role::User)), Some(Party::Buyer));
        assert_eq!(party_of(&e, &user("s", Role::User)), Some(Party::Seller));
        assert_eq!(party_of(&e, &user("op", Role::Operator)), Some(Party::Operator));
        assert_eq!(party_of(&e, &user("x", Role::User)), None);
    }

    #[test]
    fn tokens_are_long_random_and_hashed_for_storage() {
        let (a, b) = (new_token(), new_token());
        assert_ne!(a, b);
        assert_eq!(a.len(), 3 + 64);
        assert_eq!(hash_token(&a).len(), 64);
        assert_ne!(hash_token(&a), a);
    }
}
