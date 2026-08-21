//! Akroasis library interface — exposes the domain command modules
//! (radio, mesh, vault) as a typed API consumed by akroasis-server
//! and future desktop/MCP surfaces.
// WHY: command modules pre-date the lib target and have no public doc coverage;
// suppress until the library API stabilises per #118/#126 follow-up scope.
#![expect(
    missing_docs,
    reason = "command modules pre-date the lib target; doc coverage deferred to API stabilisation"
)]

pub mod caller;
pub mod mesh;
pub mod radio;
pub mod vault;
