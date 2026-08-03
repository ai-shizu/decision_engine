//! Second key of the two-key witness seal (FLV-R-7 / SPEC §3.3).
//!
//! `VerifiedFlavor` is constructible only inside this module, and only from
//! a `Checked` produced by `checked::check`. Sibling modules and the crate
//! root cannot write the private `checked` field (E0451). This module cannot
//! forge `Checked(raw)` (E0603).
//!
//! # Seal fences (FLV-I-09 / FLV-W-06)
//!
//! Each negative `compile_fail` fence has a positive companion with the
//! **same imports and scaffolding**, attacking surface removed. rustdoc does
//! not enforce error-code annotations (FLV-W-06); companions catch "wrong
//! reason" green.

use serde::ser::{Serialize, Serializer};

use crate::flavor::checked::{self, Checked};
use crate::flavor::policy::FlavorPolicy;

/// Context witness: the held string was accepted by `verify` under a policy.
///
/// Implements hand-written `Serialize` (outbound only). Must NOT implement
/// `Deserialize` / `Default` / `Clone` / `Deref` / `DerefMut` / `From<String>`
/// / `TryFrom` / `FromStr` (SPEC §3.4).
#[derive(Debug)]
pub struct VerifiedFlavor {
    checked: Checked,
}

impl VerifiedFlavor {
    /// Sole safe transition from unverified text to a witness.
    pub fn verify(raw: &str, ctx: &FlavorPolicy) -> Option<VerifiedFlavor> {
        let checked = checked::check(raw, ctx)?;
        Some(VerifiedFlavor { checked })
    }

    /// Public read as `&str` only (SPEC §3.4).
    pub fn as_str(&self) -> &str {
        self.checked.as_str()
    }
}

impl Serialize for VerifiedFlavor {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.as_str().serialize(serializer)
    }
}

// ---------------------------------------------------------------------------
// Seal probe (FLV-I-04 / §9.1-3): forge Checked from this sibling → E0603.
// ---------------------------------------------------------------------------

/// Driven by `cargo rustc --features flavor-layer --lib -- --cfg flavor_seal_probe_checked`.
#[cfg(flavor_seal_probe_checked)]
#[allow(dead_code)]
fn flavor_seal_probe_checked() {
    let _forged = crate::flavor::checked::Checked(String::from("forged"));
    let _ = _forged;
}

// ---------------------------------------------------------------------------
// Negative fences + positive companions (FLV-I-09 / FLV-W-06).
// Attack one per fence. Companion = same scaffolding, attack removed.
// Must appear *before* any `#[cfg(test)]` module (clippy: items_after_test_module).
// ---------------------------------------------------------------------------

/// Positive companion: scaffolding for Deserialize-for<'de> fence compiles.
/// ```
/// use pkb_desktop_lib::flavor::VerifiedFlavor;
/// fn assert_named(_x: &VerifiedFlavor) {}
/// ```
///
/// `VerifiedFlavor` must not implement `Deserialize` for arbitrary `'de`.
/// ```compile_fail,E0277
/// use pkb_desktop_lib::flavor::VerifiedFlavor;
/// fn assert_de<'de, T: serde::Deserialize<'de>>() {}
/// assert_de::<VerifiedFlavor>();
/// ```
fn _fence_no_deserialize_for_de() {}

/// Positive companion: scaffolding for Deserialize-'static fence compiles.
/// ```
/// use pkb_desktop_lib::flavor::VerifiedFlavor;
/// fn assert_named(_x: &VerifiedFlavor) {}
/// ```
///
/// `VerifiedFlavor` must not implement `Deserialize<'static>`.
/// ```compile_fail,E0277
/// use pkb_desktop_lib::flavor::VerifiedFlavor;
/// fn assert_de_static<T: serde::Deserialize<'static>>() {}
/// assert_de_static::<VerifiedFlavor>();
/// ```
fn _fence_no_deserialize_static() {}

/// Positive companion: scaffolding for Default fence compiles.
/// ```
/// use pkb_desktop_lib::flavor::VerifiedFlavor;
/// fn assert_named(_x: &VerifiedFlavor) {}
/// ```
///
/// `VerifiedFlavor` must not implement `Default`.
/// ```compile_fail,E0277
/// use pkb_desktop_lib::flavor::VerifiedFlavor;
/// fn assert_default<T: Default>() {}
/// assert_default::<VerifiedFlavor>();
/// ```
fn _fence_no_default() {}

/// Positive companion: scaffolding for Clone fence compiles.
/// ```
/// use pkb_desktop_lib::flavor::VerifiedFlavor;
/// fn assert_named(_x: &VerifiedFlavor) {}
/// ```
///
/// `VerifiedFlavor` must not implement `Clone`.
/// ```compile_fail,E0277
/// use pkb_desktop_lib::flavor::VerifiedFlavor;
/// fn assert_clone<T: Clone>() {}
/// assert_clone::<VerifiedFlavor>();
/// ```
fn _fence_no_clone() {}

/// Positive companion: scaffolding for DerefMut fence compiles.
/// ```
/// use pkb_desktop_lib::flavor::VerifiedFlavor;
/// fn assert_named(_x: &VerifiedFlavor) {}
/// ```
///
/// `VerifiedFlavor` must not implement `DerefMut`.
/// ```compile_fail,E0277
/// use pkb_desktop_lib::flavor::VerifiedFlavor;
/// fn assert_deref_mut<T: std::ops::DerefMut>() {}
/// assert_deref_mut::<VerifiedFlavor>();
/// ```
fn _fence_no_deref_mut() {}

/// Positive companion: scaffolding for From<String> fence compiles.
/// ```
/// use pkb_desktop_lib::flavor::VerifiedFlavor;
/// fn assert_named(_x: &VerifiedFlavor) {}
/// ```
///
/// `VerifiedFlavor` must not implement `From<String>`.
/// ```compile_fail,E0277
/// use pkb_desktop_lib::flavor::VerifiedFlavor;
/// fn assert_from_string<T: From<String>>() {}
/// assert_from_string::<VerifiedFlavor>();
/// ```
fn _fence_no_from_string() {}

/// Positive companion: scaffolding for TryFrom<String> fence compiles.
/// ```
/// use pkb_desktop_lib::flavor::VerifiedFlavor;
/// fn assert_named(_x: &VerifiedFlavor) {}
/// ```
///
/// `VerifiedFlavor` must not implement `TryFrom<String>`.
/// ```compile_fail,E0277
/// use pkb_desktop_lib::flavor::VerifiedFlavor;
/// fn assert_try_from_string<T: TryFrom<String>>() {}
/// assert_try_from_string::<VerifiedFlavor>();
/// ```
fn _fence_no_try_from_string() {}

/// Positive companion: scaffolding for FromStr fence compiles.
/// ```
/// use pkb_desktop_lib::flavor::VerifiedFlavor;
/// fn assert_named(_x: &VerifiedFlavor) {}
/// ```
///
/// `VerifiedFlavor` must not implement `FromStr`.
/// ```compile_fail,E0277
/// use pkb_desktop_lib::flavor::VerifiedFlavor;
/// fn assert_from_str<T: std::str::FromStr>() {}
/// assert_from_str::<VerifiedFlavor>();
/// ```
fn _fence_no_from_str() {}

/// Positive companion: naming the type is fine (brace attack removed).
/// ```
/// use pkb_desktop_lib::flavor::VerifiedFlavor;
/// fn assert_named(_x: &VerifiedFlavor) {}
/// ```
///
/// Brace construction of `VerifiedFlavor` from crate-external code is sealed.
/// (External crates cannot name private field `checked`; E0451 / privacy.)
/// ```compile_fail
/// use pkb_desktop_lib::flavor::VerifiedFlavor;
/// fn forge() -> VerifiedFlavor {
///     VerifiedFlavor { checked: unreachable!() }
/// }
/// ```
fn _fence_no_brace_construct() {}

/// Positive companion: naming the type is fine (struct-update attack removed).
/// ```
/// use pkb_desktop_lib::flavor::VerifiedFlavor;
/// fn assert_named(_x: &VerifiedFlavor) {}
/// ```
///
/// Struct update syntax cannot mint a new witness from an existing one
/// without going through `verify` (private field blocks `..old`).
/// ```compile_fail
/// use pkb_desktop_lib::flavor::VerifiedFlavor;
/// fn update(old: VerifiedFlavor) -> VerifiedFlavor {
///     VerifiedFlavor { ..old }
/// }
/// ```
fn _fence_no_struct_update() {}

/// Positive companion: naming the type is fine (rebuild attack removed).
/// ```
/// use pkb_desktop_lib::flavor::VerifiedFlavor;
/// fn assert_named(_x: &VerifiedFlavor) {}
/// ```
///
/// Destructure-and-rebuild is sealed (private field).
/// ```compile_fail
/// use pkb_desktop_lib::flavor::VerifiedFlavor;
/// fn rebuild(v: VerifiedFlavor) -> VerifiedFlavor {
///     let VerifiedFlavor { checked } = v;
///     VerifiedFlavor { checked }
/// }
/// ```
fn _fence_no_destructure_rebuild() {}

// ---------------------------------------------------------------------------
// Positive contract: Serialize (ordinary unit test — not a doctest fence).
// ---------------------------------------------------------------------------

#[cfg(test)]
mod serialize_contract {
    use super::VerifiedFlavor;
    use serde::Serialize;

    fn assert_serialize<T: Serialize>() {}

    #[test]
    fn verified_flavor_implements_serialize() {
        assert_serialize::<VerifiedFlavor>();
    }
}
