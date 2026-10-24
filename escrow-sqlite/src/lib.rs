//! SQLite-backed [`Store`]. The preimage has no column here by design: it is never persisted.

use std::str::FromStr;

use async_trait::async_trait;
use escrow_core::{
    model::{AuditEntry, Escrow, NodeConnection, NodeEventRecord, NodeInvoiceState, NodeKind, PaymentHash, User},
    state::EscrowState,
    store::{EscrowUpdate, Store, StoreError, UserStore},
};
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow},
    Row, SqlitePool,
};

/// Schema migrations, applied in order. The database remembers how many have run in SQLite's
/// `user_version`. Never edit a migration that has shipped: add a new one.
const MIGRATIONS: &[&str] = &[
    include_str!("migrations/001_initial.sql"),
    include_str!("migrations/002_open_offers.sql"),
    include_str!("migrations/003_own_nodes.sql"),
    include_str!("migrations/004_arbitration.sql"),
];
const ESCROW_COLUMNS: &str = "id, buyer_id, seller_id, arbiter_id, arbiter_key, arbiter_sealed, \
     arbiter_confirmed_at, node, amount_sats, description, payment_hash, \
     invoice, invoice_expires_at, state, deadline_at, created_at, updated_at";

pub struct SqliteStore {
    pool: SqlitePool,
}

fn backend(e: impl std::fmt::Display) -> StoreError {
    StoreError::Backend(e.to_string())
}

impl SqliteStore {
    /// Open (creating if needed) the database at `path` and make sure the schema exists.
    pub async fn open(path: &str) -> Result<Self, StoreError> {
        let opts = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .foreign_keys(true);
        Self::connect(opts, 5).await
    }

    /// A private in-memory database, for tests.
    pub async fn in_memory() -> Result<Self, StoreError> {
        let opts = SqliteConnectOptions::from_str("sqlite::memory:").map_err(backend)?.foreign_keys(true);
        // Every in-memory connection is its own database, so keep exactly one.
        Self::connect(opts, 1).await
    }

    async fn connect(opts: SqliteConnectOptions, max_connections: u32) -> Result<Self, StoreError> {
        let pool = SqlitePoolOptions::new()
            .max_connections(max_connections)
            .connect_with(opts)
            .await
            .map_err(backend)?;
        migrate(&pool).await?;
        Ok(Self { pool })
    }
}

/// Bring the database up to the latest schema. Each migration runs in its own transaction with
/// foreign keys off (required by SQLite to rebuild a table that others reference); the foreign
/// key check afterwards makes sure the rebuild left nothing dangling.
async fn migrate(pool: &SqlitePool) -> Result<(), StoreError> {
    let mut conn = pool.acquire().await.map_err(backend)?;
    let applied: i64 = sqlx::query_scalar("PRAGMA user_version").fetch_one(&mut *conn).await.map_err(backend)?;
    if applied as usize > MIGRATIONS.len() {
        return Err(StoreError::Backend(format!(
            "database schema version {applied} is newer than this build understands ({})",
            MIGRATIONS.len()
        )));
    }
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(applied as usize) {
        sqlx::query("PRAGMA foreign_keys = OFF").execute(&mut *conn).await.map_err(backend)?;
        let result = async {
            let mut tx = sqlx::Connection::begin(&mut *conn).await?;
            sqlx::raw_sql(sql).execute(&mut *tx).await?;
            let broken = sqlx::query("PRAGMA foreign_key_check").fetch_all(&mut *tx).await?;
            if !broken.is_empty() {
                return Err(sqlx::Error::Protocol(format!("migration {} left broken foreign keys", i + 1)));
            }
            sqlx::query(&format!("PRAGMA user_version = {}", i + 1)).execute(&mut *tx).await?;
            tx.commit().await
        }
        .await;
        sqlx::query("PRAGMA foreign_keys = ON").execute(&mut *conn).await.map_err(backend)?;
        result.map_err(backend)?;
    }
    Ok(())
}

fn hash_from_hex(s: &str) -> Result<PaymentHash, StoreError> {
    hex::decode(s)
        .map_err(backend)?
        .try_into()
        .map_err(|_| StoreError::Backend("stored payment hash is not 32 bytes".into()))
}

fn escrow_from_row(row: &SqliteRow) -> Result<Escrow, StoreError> {
    let payment_hash: Option<String> = row.try_get("payment_hash").map_err(backend)?;
    let state: String = row.try_get("state").map_err(backend)?;
    let node: String = row.try_get("node").map_err(backend)?;
    Ok(Escrow {
        id: row.try_get("id").map_err(backend)?,
        buyer_id: row.try_get("buyer_id").map_err(backend)?,
        seller_id: row.try_get("seller_id").map_err(backend)?,
        arbiter_id: row.try_get("arbiter_id").map_err(backend)?,
        arbiter_key: row.try_get("arbiter_key").map_err(backend)?,
        arbiter_sealed: row.try_get("arbiter_sealed").map_err(backend)?,
        arbiter_confirmed_at: row.try_get("arbiter_confirmed_at").map_err(backend)?,
        node: node.parse::<NodeKind>().map_err(StoreError::Backend)?,
        amount_sats: row.try_get::<i64, _>("amount_sats").map_err(backend)? as u64,
        description: row.try_get("description").map_err(backend)?,
        payment_hash: payment_hash.as_deref().map(hash_from_hex).transpose()?,
        invoice: row.try_get("invoice").map_err(backend)?,
        invoice_expires_at: row.try_get("invoice_expires_at").map_err(backend)?,
        state: state.parse().map_err(StoreError::Backend)?,
        deadline_at: row.try_get("deadline_at").map_err(backend)?,
        created_at: row.try_get("created_at").map_err(backend)?,
        updated_at: row.try_get("updated_at").map_err(backend)?,
    })
}

fn audit_from_row(row: &SqliteRow) -> Result<AuditEntry, StoreError> {
    let from: String = row.try_get("from_state").map_err(backend)?;
    let to: String = row.try_get("to_state").map_err(backend)?;
    Ok(AuditEntry {
        escrow_id: row.try_get("escrow_id").map_err(backend)?,
        actor: row.try_get("actor").map_err(backend)?,
        action: row.try_get("action").map_err(backend)?,
        from_state: from.parse().map_err(StoreError::Backend)?,
        to_state: to.parse().map_err(StoreError::Backend)?,
        reason: row.try_get("reason").map_err(backend)?,
        created_at: row.try_get("created_at").map_err(backend)?,
    })
}

fn node_state_from_str(s: &str) -> Result<NodeInvoiceState, StoreError> {
    [
        NodeInvoiceState::Open,
        NodeInvoiceState::Accepted,
        NodeInvoiceState::Settled,
        NodeInvoiceState::Canceled,
    ]
    .into_iter()
    .find(|st| st.as_str() == s)
    .ok_or_else(|| StoreError::Backend(format!("unknown node invoice state '{s}'")))
}

#[async_trait]
impl Store for SqliteStore {
    async fn insert(&self, e: Escrow) -> Result<(), StoreError> {
        let res = sqlx::query(&format!(
            "INSERT INTO escrows ({ESCROW_COLUMNS}) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        ))
        .bind(&e.id)
        .bind(&e.buyer_id)
        .bind(&e.seller_id)
        .bind(&e.arbiter_id)
        .bind(&e.arbiter_key)
        .bind(&e.arbiter_sealed)
        .bind(e.arbiter_confirmed_at)
        .bind(e.node.as_str())
        .bind(e.amount_sats as i64)
        .bind(&e.description)
        .bind(e.payment_hash.map(hex::encode))
        .bind(&e.invoice)
        .bind(e.invoice_expires_at)
        .bind(e.state.as_str())
        .bind(e.deadline_at)
        .bind(e.created_at)
        .bind(e.updated_at)
        .execute(&self.pool)
        .await;
        match res {
            Ok(_) => Ok(()),
            Err(sqlx::Error::Database(db)) if db.is_unique_violation() => Err(StoreError::Conflict),
            Err(e) => Err(backend(e)),
        }
    }

    async fn get(&self, id: &str) -> Result<Option<Escrow>, StoreError> {
        let row = sqlx::query(&format!("SELECT {ESCROW_COLUMNS} FROM escrows WHERE id = ?"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(backend)?;
        row.as_ref().map(escrow_from_row).transpose()
    }

    async fn get_by_hash(&self, hash: &PaymentHash) -> Result<Option<Escrow>, StoreError> {
        let row = sqlx::query(&format!("SELECT {ESCROW_COLUMNS} FROM escrows WHERE payment_hash = ?"))
            .bind(hex::encode(hash))
            .fetch_optional(&self.pool)
            .await
            .map_err(backend)?;
        row.as_ref().map(escrow_from_row).transpose()
    }

    async fn list_active(&self) -> Result<Vec<Escrow>, StoreError> {
        let terminal: Vec<&str> = EscrowState::ALL
            .into_iter()
            .filter(|s| s.is_terminal())
            .map(EscrowState::as_str)
            .collect();
        let marks = vec!["?"; terminal.len()].join(", ");
        let sql = format!(
            "SELECT {ESCROW_COLUMNS} FROM escrows WHERE state NOT IN ({marks}) ORDER BY created_at, id"
        );
        let mut q = sqlx::query(&sql);
        for s in terminal {
            q = q.bind(s);
        }
        let rows = q.fetch_all(&self.pool).await.map_err(backend)?;
        rows.iter().map(escrow_from_row).collect()
    }

    async fn list_for_user(&self, user_id: &str) -> Result<Vec<Escrow>, StoreError> {
        let rows = sqlx::query(&format!(
            "SELECT {ESCROW_COLUMNS} FROM escrows WHERE buyer_id = ?1 OR seller_id = ?1 OR arbiter_id = ?1 \
             ORDER BY created_at DESC, id ASC LIMIT 100"
        ))
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        rows.iter().map(escrow_from_row).collect()
    }

    async fn transition(
        &self,
        id: &str,
        expected: EscrowState,
        update: EscrowUpdate,
        audit: AuditEntry,
    ) -> Result<Escrow, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;

        // Compare-and-set on the state: only one concurrent actor can move the escrow.
        let res = sqlx::query(
            "UPDATE escrows SET state = ?, updated_at = ?, \
                 buyer_id = COALESCE(?, buyer_id), \
                 payment_hash = COALESCE(?, payment_hash), \
                 invoice = COALESCE(?, invoice), \
                 invoice_expires_at = COALESCE(?, invoice_expires_at), \
                 arbiter_sealed = COALESCE(?, arbiter_sealed), \
                 arbiter_confirmed_at = COALESCE(?, arbiter_confirmed_at) \
             WHERE id = ? AND state = ?",
        )
        .bind(audit.to_state.as_str())
        .bind(audit.created_at)
        .bind(&update.buyer_id)
        .bind(update.payment_hash.map(hex::encode))
        .bind(&update.invoice)
        .bind(update.invoice_expires_at)
        .bind(&update.arbiter_sealed)
        .bind(update.arbiter_confirmed_at)
        .bind(id)
        .bind(expected.as_str())
        .execute(&mut *tx)
        .await;
        let rows = match res {
            Ok(r) => r.rows_affected(),
            Err(sqlx::Error::Database(db)) if db.is_unique_violation() => return Err(StoreError::Conflict),
            Err(e) => return Err(backend(e)),
        };
        if rows == 0 {
            let exists = sqlx::query("SELECT 1 FROM escrows WHERE id = ?")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(backend)?
                .is_some();
            return Err(if exists { StoreError::Conflict } else { StoreError::NotFound });
        }

        sqlx::query(
            "INSERT INTO audit_log (escrow_id, actor, action, from_state, to_state, reason, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&audit.escrow_id)
        .bind(&audit.actor)
        .bind(&audit.action)
        .bind(audit.from_state.as_str())
        .bind(audit.to_state.as_str())
        .bind(&audit.reason)
        .bind(audit.created_at)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;

        let row = sqlx::query(&format!("SELECT {ESCROW_COLUMNS} FROM escrows WHERE id = ?"))
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .map_err(backend)?;
        let escrow = escrow_from_row(&row)?;
        tx.commit().await.map_err(backend)?;
        Ok(escrow)
    }

    async fn audit_log(&self, id: &str) -> Result<Vec<AuditEntry>, StoreError> {
        let rows = sqlx::query(
            "SELECT escrow_id, actor, action, from_state, to_state, reason, created_at \
             FROM audit_log WHERE escrow_id = ? ORDER BY seq",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        rows.iter().map(audit_from_row).collect()
    }

    async fn record_node_event(&self, ev: NodeEventRecord) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO node_events (escrow_id, node_invoice_state, raw_event_hash, received_at) \
             VALUES (?, ?, ?, ?)",
        )
        .bind(&ev.escrow_id)
        .bind(ev.node_invoice_state.as_str())
        .bind(&ev.raw_event_hash)
        .bind(ev.received_at)
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        Ok(())
    }

    async fn node_events(&self, id: &str) -> Result<Vec<NodeEventRecord>, StoreError> {
        let rows = sqlx::query(
            "SELECT escrow_id, node_invoice_state, raw_event_hash, received_at \
             FROM node_events WHERE escrow_id = ? ORDER BY seq",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        rows.iter()
            .map(|r| {
                let state: String = r.try_get("node_invoice_state").map_err(backend)?;
                Ok(NodeEventRecord {
                    escrow_id: r.try_get("escrow_id").map_err(backend)?,
                    node_invoice_state: node_state_from_str(&state)?,
                    raw_event_hash: r.try_get("raw_event_hash").map_err(backend)?,
                    received_at: r.try_get("received_at").map_err(backend)?,
                })
            })
            .collect()
    }
}

fn node_connection_from_row(row: &SqliteRow) -> Result<NodeConnection, StoreError> {
    Ok(NodeConnection {
        user_id: row.try_get("user_id").map_err(backend)?,
        address: row.try_get("address").map_err(backend)?,
        tls_cert: row.try_get("tls_cert").map_err(backend)?,
        macaroon_enc: row.try_get("macaroon_enc").map_err(backend)?,
        updated_at: row.try_get("updated_at").map_err(backend)?,
    })
}

fn user_from_row(row: &SqliteRow) -> Result<User, StoreError> {
    let role: String = row.try_get("role").map_err(backend)?;
    Ok(User {
        id: row.try_get("id").map_err(backend)?,
        role: role.parse().map_err(StoreError::Backend)?,
        created_at: row.try_get("created_at").map_err(backend)?,
    })
}

#[async_trait]
impl UserStore for SqliteStore {
    async fn insert_user(&self, user: User, token_hash: String) -> Result<(), StoreError> {
        let res = sqlx::query("INSERT INTO users (id, role, token_hash, created_at) VALUES (?, ?, ?, ?)")
            .bind(&user.id)
            .bind(user.role.as_str())
            .bind(token_hash)
            .bind(user.created_at)
            .execute(&self.pool)
            .await;
        match res {
            Ok(_) => Ok(()),
            Err(sqlx::Error::Database(db)) if db.is_unique_violation() => Err(StoreError::Conflict),
            Err(e) => Err(backend(e)),
        }
    }

    async fn user_by_token_hash(&self, token_hash: &str) -> Result<Option<User>, StoreError> {
        let row = sqlx::query("SELECT id, role, created_at FROM users WHERE token_hash = ?")
            .bind(token_hash)
            .fetch_optional(&self.pool)
            .await
            .map_err(backend)?;
        row.as_ref().map(user_from_row).transpose()
    }

    async fn get_user(&self, id: &str) -> Result<Option<User>, StoreError> {
        let row = sqlx::query("SELECT id, role, created_at FROM users WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(backend)?;
        row.as_ref().map(user_from_row).transpose()
    }

    async fn put_arbiter_key(&self, user_id: &str, public_key: &str, at: i64) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO arbiter_keys (user_id, public_key, updated_at) VALUES (?, ?, ?) \
             ON CONFLICT(user_id) DO UPDATE SET public_key = excluded.public_key, updated_at = excluded.updated_at",
        )
        .bind(user_id)
        .bind(public_key)
        .bind(at)
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        Ok(())
    }

    async fn arbiter_key(&self, user_id: &str) -> Result<Option<String>, StoreError> {
        let row = sqlx::query("SELECT public_key FROM arbiter_keys WHERE user_id = ?")
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(backend)?;
        row.map(|r| r.try_get("public_key").map_err(backend)).transpose()
    }

    async fn put_node_connection(&self, c: NodeConnection) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO node_connections (user_id, address, tls_cert, macaroon_enc, updated_at) \
             VALUES (?, ?, ?, ?, ?) \
             ON CONFLICT(user_id) DO UPDATE SET address = excluded.address, tls_cert = excluded.tls_cert, \
                 macaroon_enc = excluded.macaroon_enc, updated_at = excluded.updated_at",
        )
        .bind(&c.user_id)
        .bind(&c.address)
        .bind(&c.tls_cert)
        .bind(&c.macaroon_enc)
        .bind(c.updated_at)
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        Ok(())
    }

    async fn node_connection(&self, user_id: &str) -> Result<Option<NodeConnection>, StoreError> {
        let row = sqlx::query(
            "SELECT user_id, address, tls_cert, macaroon_enc, updated_at FROM node_connections WHERE user_id = ?",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        row.as_ref().map(node_connection_from_row).transpose()
    }

    async fn delete_node_connection(&self, user_id: &str) -> Result<bool, StoreError> {
        let res = sqlx::query("DELETE FROM node_connections WHERE user_id = ?")
            .bind(user_id)
            .execute(&self.pool)
            .await
            .map_err(backend)?;
        Ok(res.rows_affected() > 0)
    }
}
