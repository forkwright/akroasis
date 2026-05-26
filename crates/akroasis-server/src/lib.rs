//! Typed axum HTTP backend — the callable API surface for akroasis-desktop
//! and any --json / MCP client that needs a durable service interface.
//!
//! # API surface (v1)
//!
//! All responses are JSON with a `schema_version` field matching the CLI
//! `--json` reports. Endpoints mirror the CLI subcommands one-to-one so
//! desktop frontends and agents can drive the same logic without forking.
//!
//! | Method | Path | CLI equivalent |
//! |--------|------|----------------|
//! | GET | `/api/v1/radio/detect` | `radio detect --json` |
//! | GET | `/api/v1/mesh/status` | `mesh status --json` |
//! | GET | `/api/v1/mesh/nodes` | `mesh nodes --json` |
//! | GET | `/api/v1/mesh/topology` | `mesh topology --json` |

#![deny(missing_docs)]

pub mod error;
pub mod mesh;
pub mod radio;
pub mod router;
pub mod server;

pub use server::serve;
