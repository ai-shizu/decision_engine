//! BLACKBOX SIMULATOR — Tauri command layer + sim worker thread (Phase 5,
//! SPEC §15). See `handle.rs`'s module header for why this is the one place
//! (besides `db::blackbox_repo`) allowed to import both `blackbox_sim` and
//! `db`.

pub(crate) mod commands;
pub(crate) mod handle;
pub(crate) mod view;

#[cfg(feature = "flavor-live")]
pub(crate) mod flavor_slot;
