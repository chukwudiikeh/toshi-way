//! Escrow core: state machine, timing rules and service logic.
//!
//! Nothing here depends on a running node, a database or a web server. The node sits behind
//! [`adapter::LightningAdapter`] and persistence behind [`store::Store`], each with an
//! in-memory implementation for tests.

pub mod adapter;
pub mod mock;
pub mod model;
pub mod service;
pub mod state;
pub mod store;
pub mod testing;
pub mod timing;
