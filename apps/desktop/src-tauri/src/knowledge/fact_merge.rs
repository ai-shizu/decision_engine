//! Pure, field-level CompanyFacts merge policy (Step 10).

use super::edinet_client::{normalize_filer_key, CompanyFacts, EdinetFactAcquisition};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FactOrigin {
    Unknown,
    Wikipedia,
    Edinet,
    UnknownProtected,
    Manual,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FactStorage {
    Session,
    Vault,
    Live,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FactCell {
    pub value: String,
    pub origin: FactOrigin,
    pub storage: FactStorage,
    pub doc_id: Option<String>,
    pub submitted_at: Option<String>,
    pub fetched_at: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FactCellWire {
    pub field: String,
    pub value: String,
    pub origin: FactOriginWire,
    pub storage: FactStorageWire,
    pub doc_id: Option<String>,
    pub submitted_at: Option<String>,
    pub fetched_at: Option<i64>,
    pub revision: i64,
    pub schema_version: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactOriginWire {
    Manual,
    Wikipedia,
    Edinet,
    UnknownProtected,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactStorageWire {
    Session,
    Vault,
    Live,
}

impl From<FactOrigin> for FactOriginWire {
    fn from(v: FactOrigin) -> Self {
        match v {
            FactOrigin::Manual => Self::Manual,
            FactOrigin::Wikipedia => Self::Wikipedia,
            FactOrigin::Edinet => Self::Edinet,
            FactOrigin::UnknownProtected => Self::UnknownProtected,
            FactOrigin::Unknown => Self::Unknown,
        }
    }
}
impl From<FactStorage> for FactStorageWire {
    fn from(v: FactStorage) -> Self {
        match v {
            FactStorage::Session => Self::Session,
            FactStorage::Vault => Self::Vault,
            FactStorage::Live => Self::Live,
        }
    }
}
impl From<FactOriginWire> for FactOrigin {
    fn from(v: FactOriginWire) -> Self {
        match v {
            FactOriginWire::Manual => Self::Manual,
            FactOriginWire::Wikipedia => Self::Wikipedia,
            FactOriginWire::Edinet => Self::Edinet,
            FactOriginWire::UnknownProtected => Self::UnknownProtected,
            FactOriginWire::Unknown => Self::Unknown,
        }
    }
}
impl From<FactStorageWire> for FactStorage {
    fn from(v: FactStorageWire) -> Self {
        match v {
            FactStorageWire::Session => Self::Session,
            FactStorageWire::Vault => Self::Vault,
            FactStorageWire::Live => Self::Live,
        }
    }
}

pub fn cell_to_wire(
    field: &str,
    cell: &FactCell,
    revision: i64,
    schema_version: i64,
) -> FactCellWire {
    FactCellWire {
        field: field.into(),
        value: cell.value.clone(),
        origin: cell.origin.into(),
        storage: cell.storage.into(),
        doc_id: cell.doc_id.clone(),
        submitted_at: cell.submitted_at.clone(),
        fetched_at: cell.fetched_at,
        revision,
        schema_version,
    }
}
pub fn cell_from_wire(wire: &FactCellWire) -> FactCell {
    FactCell {
        value: wire.value.clone(),
        origin: wire.origin.into(),
        storage: wire.storage.into(),
        doc_id: wire.doc_id.clone(),
        submitted_at: wire.submitted_at.clone(),
        fetched_at: wire.fetched_at,
    }
}

/// Convert a confirmed acquisition while carrying the selected filing
/// provenance onto every adopted EDINET field.
pub fn cells_from_edinet_acquisition(acquisition: &EdinetFactAcquisition) -> FactCells {
    let facts = acquisition.facts();
    let mut out = FactCells::new();
    let fields = [
        ("company_name", &facts.company_name, true),
        ("edinet_code", &facts.edinet_code, true),
        ("doc_id", &facts.doc_id, true),
        (
            "business_summary",
            &facts.business_summary,
            acquisition.fields().business_summary,
        ),
        (
            "business_risks",
            &facts.business_risks,
            acquisition.fields().business_risks,
        ),
        (
            "performance_summary",
            &facts.performance_summary,
            acquisition.fields().performance_summary,
        ),
    ];
    for (field, value, adopted) in fields {
        if adopted && !value.trim().is_empty() {
            out.insert(
                field.into(),
                FactCell {
                    value: value.clone(),
                    origin: FactOrigin::Edinet,
                    storage: FactStorage::Live,
                    doc_id: Some(acquisition.selected_doc_id().to_string()),
                    submitted_at: Some(acquisition.submitted_at().to_string()),
                    fetched_at: None,
                },
            );
        }
    }
    out
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum SubjectKey {
    Name(String),
    Edinet(String),
}

impl TryFrom<String> for SubjectKey {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let (kind, key) = value.split_once(':').ok_or("invalid subject key")?;
        if key.trim().is_empty() || key.contains(':') {
            return Err("invalid subject key value");
        }
        match kind {
            "name" if normalize_filer_key(key) == key => Ok(Self::Name(key.to_string())),
            "edinet"
                if key.len() == 6
                    && key.starts_with('E')
                    && key[1..].chars().all(|c| c.is_ascii_digit()) =>
            {
                Ok(Self::Edinet(key.to_string()))
            }
            _ => Err("invalid subject key"),
        }
    }
}
impl From<SubjectKey> for String {
    fn from(value: SubjectKey) -> Self {
        match value {
            SubjectKey::Name(k) => format!("name:{k}"),
            SubjectKey::Edinet(k) => format!("edinet:{k}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SubjectTransition {
    Rekey { from: SubjectKey, to: SubjectKey },
    Switch { from: SubjectKey, to: SubjectKey },
}

pub type FactCells = std::collections::BTreeMap<String, FactCell>;

fn nonempty(value: &str) -> bool {
    !value.trim().is_empty()
}

fn fresher(existing: &FactCell, incoming: &FactCell) -> bool {
    match (existing.origin, incoming.origin) {
        (FactOrigin::Edinet, FactOrigin::Edinet) => {
            match (&existing.submitted_at, &incoming.submitted_at) {
                (Some(a), Some(b)) => b > a,
                _ => false,
            }
        }
        (FactOrigin::Wikipedia, FactOrigin::Wikipedia) => {
            match (existing.fetched_at, incoming.fetched_at) {
                (Some(a), Some(b)) => b > a,
                _ => false,
            }
        }
        _ => false,
    }
}

fn rank(origin: FactOrigin) -> u8 {
    match origin {
        FactOrigin::Unknown => 0,
        FactOrigin::Wikipedia => 1,
        FactOrigin::Edinet => 2,
        FactOrigin::UnknownProtected | FactOrigin::Manual => 3,
    }
}
fn wins(existing: &FactCell, incoming: &FactCell) -> bool {
    if !nonempty(&incoming.value) {
        return false;
    }
    if existing.origin == FactOrigin::UnknownProtected {
        return false;
    }
    if !nonempty(&existing.value) && existing.origin == FactOrigin::Unknown {
        return true;
    }
    if rank(incoming.origin) > rank(existing.origin) {
        return true;
    }
    incoming.origin == existing.origin && fresher(existing, incoming)
}

/// Explicit user edit path; unlike enrichment it may replace protected values.
pub fn user_patch(base: &FactCells, patch: &FactCells) -> FactCells {
    let mut out = base.clone();
    for (field, value) in patch {
        let manual = FactCell {
            value: value.value.clone(),
            origin: FactOrigin::Manual,
            storage: FactStorage::Session,
            doc_id: None,
            submitted_at: None,
            fetched_at: None,
        };
        out.insert(field.clone(), manual);
    }
    out
}

/// Merge cells without mutating either input. An empty EDINET value never wins.
pub fn merge_fact_cells(base: &FactCells, incoming: &FactCells) -> FactCells {
    let mut out = base.clone();
    for (field, next) in incoming {
        if !nonempty(&next.value) || next.origin == FactOrigin::Manual {
            continue;
        }
        match out.get(field) {
            Some(current) if !wins(current, next) => {}
            _ => {
                out.insert(field.clone(), next.clone());
            }
        }
    }
    out
}

/// Merge two already-authoritative snapshots during an identity-preserving
/// rekey. Unlike enrichment, Manual cells are valid inputs here.
pub fn merge_rekey_snapshots(base: &FactCells, incoming: &FactCells) -> FactCells {
    let mut out = base.clone();
    for (field, next) in incoming {
        if !nonempty(&next.value) {
            continue;
        }
        match out.get(field) {
            Some(current)
                if rank(next.origin) < rank(current.origin)
                    || (rank(next.origin) == rank(current.origin)
                        && next.origin != current.origin)
                    || (next.origin == current.origin && !fresher(current, next)) => {}
            _ => {
                out.insert(field.clone(), next.clone());
            }
        }
    }
    out
}

/// Explicitly promotes legacy non-empty display facts to protected cells.
pub fn protected_cells_from_company_facts(facts: &CompanyFacts) -> FactCells {
    let mut out = FactCells::new();
    for (field, value) in [
        ("company_name", &facts.company_name),
        ("edinet_code", &facts.edinet_code),
        ("doc_id", &facts.doc_id),
        ("business_summary", &facts.business_summary),
        ("business_risks", &facts.business_risks),
        ("performance_summary", &facts.performance_summary),
    ] {
        if nonempty(value) {
            out.insert(
                field.into(),
                FactCell {
                    value: value.clone(),
                    origin: FactOrigin::UnknownProtected,
                    storage: FactStorage::Session,
                    doc_id: None,
                    submitted_at: None,
                    fetched_at: None,
                },
            );
        }
    }
    out
}

/// On a subject switch retain only Manual values; rekey retains all fields.
pub fn apply_subject_transition(
    cells: &FactCells,
    transition: Option<&SubjectTransition>,
) -> FactCells {
    match transition {
        Some(SubjectTransition::Switch { from, to }) if from != to => cells
            .iter()
            .filter(|(field, cell)| *field == "company_name" && cell.origin == FactOrigin::Manual)
            .map(|(field, cell)| (field.clone(), cell.clone()))
            .collect(),
        _ => cells.clone(),
    }
}

fn cell(cells: &FactCells, field: &str, fallback: &str) -> String {
    cells
        .get(field)
        .map(|c| c.value.clone())
        .filter(|v| nonempty(v))
        .unwrap_or_else(|| fallback.to_string())
}

/// Derive the display contract. Evidence text is intentionally not an input.
pub fn display_company_facts(cells: &FactCells, fallback: &CompanyFacts) -> CompanyFacts {
    let mut facts = CompanyFacts {
        company_name: cell(cells, "company_name", ""),
        edinet_code: cell(cells, "edinet_code", ""),
        doc_id: cell(cells, "doc_id", ""),
        business_summary: cell(cells, "business_summary", ""),
        business_risks: cell(cells, "business_risks", ""),
        performance_summary: cell(cells, "performance_summary", ""),
        source: String::new(),
    };
    let display_fields = [
        "company_name",
        "edinet_code",
        "doc_id",
        "business_summary",
        "business_risks",
        "performance_summary",
    ];
    let order = [
        (FactOrigin::Manual, "manual"),
        (FactOrigin::Edinet, "edinet"),
        (FactOrigin::Wikipedia, "wikipedia"),
        (FactOrigin::UnknownProtected, "unknown_protected"),
        (FactOrigin::Unknown, "unknown"),
    ];
    facts.source = order
        .iter()
        .filter(|(origin, _)| {
            display_fields.iter().any(|field| {
                cells
                    .get(*field)
                    .is_some_and(|c| c.origin == *origin && nonempty(&c.value))
            })
        })
        .map(|(_, label)| *label)
        .collect::<Vec<_>>()
        .join("+");
    let _ = fallback; // fallback is intentionally not used for managed fields
    facts
}

#[cfg(test)]
mod tests {
    use super::*;
    fn c(value: &str, origin: FactOrigin) -> FactCell {
        FactCell {
            value: value.into(),
            origin,
            storage: FactStorage::Live,
            doc_id: None,
            submitted_at: None,
            fetched_at: None,
        }
    }

    #[test]
    fn priority_and_empty_protection() {
        let mut base = FactCells::new();
        base.insert("business_risks".into(), c("wiki", FactOrigin::Wikipedia));
        let mut next = FactCells::new();
        next.insert("business_risks".into(), c("edinet", FactOrigin::Edinet));
        assert_eq!(
            merge_fact_cells(&base, &next)["business_risks"].value,
            "edinet"
        );
        next.insert("business_risks".into(), c("", FactOrigin::Edinet));
        assert_eq!(
            merge_fact_cells(&base, &next)["business_risks"].value,
            "wiki"
        );
    }

    #[test]
    fn manual_enrichment_is_rejected() {
        let mut base = FactCells::new();
        base.insert("business_summary".into(), c("edinet", FactOrigin::Edinet));
        let mut incoming = FactCells::new();
        incoming.insert("business_summary".into(), c("manual", FactOrigin::Manual));
        assert_eq!(
            merge_fact_cells(&base, &incoming)["business_summary"].value,
            "edinet"
        );
    }

    #[test]
    fn empty_new_edinet_does_not_block_wikipedia() {
        let mut empty = FactCells::new();
        empty.insert("business_summary".into(), c("", FactOrigin::Edinet));
        let merged = merge_fact_cells(&FactCells::new(), &empty);
        assert!(!merged.contains_key("business_summary"));
        let mut wiki = FactCells::new();
        wiki.insert("business_summary".into(), c("wiki", FactOrigin::Wikipedia));
        assert_eq!(
            merge_fact_cells(&merged, &wiki)["business_summary"].value,
            "wiki"
        );
    }

    #[test]
    fn subject_switch_discards_automatic_cells() {
        let mut cells = FactCells::new();
        cells.insert("business_summary".into(), c("auto", FactOrigin::Edinet));
        cells.insert("business_risks".into(), c("manual", FactOrigin::Manual));
        let t = SubjectTransition::Switch {
            from: SubjectKey::Name("a".into()),
            to: SubjectKey::Edinet("b".into()),
        };
        assert!(!apply_subject_transition(&cells, Some(&t)).contains_key("business_risks"));
        assert!(!apply_subject_transition(&cells, Some(&t)).contains_key("business_summary"));
    }

    #[test]
    fn same_origin_uses_freshness() {
        let mut a = FactCells::new();
        let mut b = FactCells::new();
        let mut old = c("old", FactOrigin::Edinet);
        old.submitted_at = Some("2024-01-01".into());
        let mut new = c("new", FactOrigin::Edinet);
        new.submitted_at = Some("2024-02-01".into());
        a.insert("business_summary".into(), old);
        b.insert("business_summary".into(), new);
        assert_eq!(merge_fact_cells(&a, &b)["business_summary"].value, "new");
    }

    #[test]
    fn protected_value_requires_user_patch() {
        let mut base = FactCells::new();
        base.insert(
            "business_risks".into(),
            c("protected", FactOrigin::UnknownProtected),
        );
        let mut auto = FactCells::new();
        auto.insert("business_risks".into(), c("manual", FactOrigin::Manual));
        assert_eq!(
            merge_fact_cells(&base, &auto)["business_risks"].value,
            "protected"
        );
        assert_eq!(user_patch(&base, &auto)["business_risks"].value, "manual");
    }

    #[test]
    fn user_patch_clears_value_and_external_provenance() {
        let mut base = FactCells::new();
        let mut old = c("edinet", FactOrigin::Edinet);
        old.doc_id = Some("doc".into());
        old.submitted_at = Some("2024-01-01".into());
        base.insert("business_summary".into(), old);
        let mut patch = FactCells::new();
        patch.insert("business_summary".into(), c("", FactOrigin::Edinet));
        let cell = &user_patch(&base, &patch)["business_summary"];
        assert!(cell.value.is_empty());
        assert_eq!(cell.origin, FactOrigin::Manual);
        assert_eq!(cell.storage, FactStorage::Session);
        assert!(cell.doc_id.is_none() && cell.submitted_at.is_none() && cell.fetched_at.is_none());
    }

    #[test]
    fn legacy_facts_convert_all_six_fields() {
        let facts = CompanyFacts {
            company_name: "A".into(),
            edinet_code: "E12345".into(),
            doc_id: "D".into(),
            business_summary: "B".into(),
            business_risks: "R".into(),
            performance_summary: "P".into(),
            source: "wikipedia".into(),
        };
        let cells = protected_cells_from_company_facts(&facts);
        assert_eq!(cells.len(), 6);
        assert!(cells
            .values()
            .all(|c| c.origin == FactOrigin::UnknownProtected));
    }

    #[test]
    fn acquisition_cells_share_selected_provenance_and_respect_flags() {
        let facts = CompanyFacts {
            company_name: "A".into(),
            edinet_code: "E12345".into(),
            doc_id: "DOC".into(),
            business_summary: "B".into(),
            business_risks: "R".into(),
            performance_summary: "P".into(),
            source: "edinet_list".into(),
        };
        let acquisition = EdinetFactAcquisition::new(
            facts,
            "DOC".into(),
            "2024-01-01 00:00".into(),
            "E12345".into(),
            crate::knowledge::edinet_client::EdinetFieldAcquisition {
                business_summary: true,
                business_risks: false,
                performance_summary: true,
            },
        )
        .unwrap();
        let cells = cells_from_edinet_acquisition(&acquisition);
        assert!(!cells.contains_key("business_risks"));
        for field in [
            "company_name",
            "edinet_code",
            "doc_id",
            "business_summary",
            "performance_summary",
        ] {
            assert_eq!(cells[field].doc_id.as_deref(), Some("DOC"));
            assert_eq!(
                cells[field].submitted_at.as_deref(),
                Some("2024-01-01 00:00")
            );
        }
    }

    #[test]
    fn subject_wire_and_source_summary_are_strict() {
        assert!(SubjectKey::try_from("bogus:x".to_string()).is_err());
        assert!(SubjectKey::try_from("name:Unnormalized:extra".to_string()).is_err());
        assert!(SubjectKey::try_from("edinet:b".to_string()).is_err());
        assert!(SubjectKey::try_from("edinet:E123456".to_string()).is_err());
        let key = SubjectKey::try_from("edinet:E12345".to_string()).unwrap();
        assert_eq!(String::from(key), "edinet:E12345");
        let transition = SubjectTransition::Rekey {
            from: SubjectKey::Name("normalized".into()),
            to: SubjectKey::Edinet("E12345".into()),
        };
        let wire = serde_json::to_string(&transition).unwrap();
        assert_eq!(
            serde_json::from_str::<SubjectTransition>(&wire).unwrap(),
            transition
        );
        let mut cells = FactCells::new();
        cells.insert("business_summary".into(), c("x", FactOrigin::Edinet));
        cells.insert("business_risks".into(), c("y", FactOrigin::Edinet));
        cells.insert("internal_only".into(), c("z", FactOrigin::Wikipedia));
        assert_eq!(
            display_company_facts(&cells, &CompanyFacts::default()).source,
            "edinet"
        );
    }

    #[test]
    fn stale_missing_timestamp_keeps_existing() {
        let mut a = FactCells::new();
        let mut b = FactCells::new();
        let mut old = c("old", FactOrigin::Wikipedia);
        old.fetched_at = None;
        let mut next = c("new", FactOrigin::Wikipedia);
        next.fetched_at = Some(2);
        a.insert("business_summary".into(), old);
        b.insert("business_summary".into(), next);
        assert_eq!(merge_fact_cells(&a, &b)["business_summary"].value, "old");
    }
}
