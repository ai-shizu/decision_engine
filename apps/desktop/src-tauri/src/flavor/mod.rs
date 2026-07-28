//! LLM Flavor Layer — type skeleton and witness seal (F-1).
//!
//! Parent: `docs/SPEC_FLAVOR_LAYER.md` v2 (FLV-R-6 / FLV-R-7).
//! Placement: top-level `flavor/` (NOT under `llm/`) so the seal can be
//! proven without pulling llama-cpp-2 / cmake (F-1 invariant).
//!
//! F-1 ships types + two-key seal + adversarial corpus only.
//! No LLM calls. Guard always returns `None`. Arena wiring is F-3.

#![deny(unsafe_code)]

pub mod checked;
pub mod corpus;
pub mod policy;
pub mod request;
pub mod verified;
pub mod wire;

pub use policy::FlavorPolicy;
pub use request::{
    FlavorLocale, FlavorRequest, FlavorSchema, FlavorSlot, SlotId, SlotValue, TemplateId,
};
pub use verified::VerifiedFlavor;
pub use wire::UnverifiedFlavor;
