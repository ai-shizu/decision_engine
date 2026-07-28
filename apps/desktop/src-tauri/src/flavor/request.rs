//! Closed-enum request vocabulary (FLV-R-6 / FLV-I-02 / FLV-I-10).
//!
//! `SlotValue` holds qualitative tags only — no numeric payloads, no free
//! `String`, no `from_quantized`. Authority types must not gain
//! `Into<SlotValue>` / `From<...> for SlotValue` (FLV-I-03).

use serde::{Deserialize, Serialize};

/// Schema marker for flavor requests (closed; no free string).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FlavorSchema {
    #[serde(rename = "flavor_request.v1")]
    V1,
}

/// Locale for flavor generation (closed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlavorLocale {
    Ja,
}

/// Template identity — closed enum; string IDs are rejected by the type system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateId {
    ArenaEventHeadline,
    ArenaEventAside,
    SettlementRipple,
}

impl TemplateId {
    /// Per-template char budget (FLV-R-5). Lives on the enum so FlavorRequest
    /// carries no numeric field (FLV-I-10).
    pub const fn max_chars(self) -> u16 {
        match self {
            TemplateId::ArenaEventHeadline => 48,
            TemplateId::ArenaEventAside => 72,
            TemplateId::SettlementRipple => 64,
        }
    }
}

/// Slot identity — closed enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlotId {
    LossSeverity,
    Trend,
    Phase,
    Mood,
}

/// Qualitative tag only. **No numeric / String-holding variants** (FLV-I-02).
///
/// Examples map to SPEC §3.2 (`LossSeverity::High` → `LossSeverityHigh`, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlotValue {
    LossSeverityHigh,
    LossSeverityModerate,
    LossSeverityLow,
    TrendRising,
    TrendFalling,
    TrendFlat,
    PhaseSettlement,
    PhaseExpansion,
    PhaseContraction,
    MoodTense,
    MoodCalm,
    MoodVolatile,
}

/// One qualitative slot binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FlavorSlot {
    pub id: SlotId,
    pub tag: SlotValue,
}

/// Flavor generation request (Rust → LLM). No numeric Fact fields (FLV-I-10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FlavorRequest {
    pub schema: FlavorSchema,
    pub template_id: TemplateId,
    pub slots: Vec<FlavorSlot>,
    pub locale: FlavorLocale,
}

impl FlavorRequest {
    pub fn max_chars(&self) -> u16 {
        self.template_id.max_chars()
    }
}
