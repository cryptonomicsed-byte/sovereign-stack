//! Google A2A v1.0 subset — Agent-to-Agent task protocol for Sovereign nodes.
//!
//! Implements the minimal A2A surface needed for inter-node task delegation:
//!
//!   GET  /a2a/agent          — AgentCard (capabilities advertisement)
//!   POST /a2a/tasks          — submit a new Task to this agent
//!   GET  /a2a/tasks/:id      — poll Task status
//!   POST /a2a/tasks/:id/cancel — cancel a running Task
//!
//! A2A Task lifecycle: submitted → working → completed | failed | canceled
//!
//! Integration: mount `a2a_router(state)` into an existing axum Router.
//! The AgentCard is built from `A2aConfig`; tasks are handled by the
//! registered `TaskHandler` trait object.

pub mod types;
pub mod router;
pub mod client;

pub use types::*;
pub use router::{a2a_router, A2aState, A2aDispatchRequest};
pub use client::A2aClient;
