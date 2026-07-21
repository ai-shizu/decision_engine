use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

// One MiB is reserved for JSON expansion/peer fields, followed by another MiB
// for the IPC id/cid/cmd envelope and terminating newline.
pub(crate) const MAX_TEXT_BYTES: usize = 6 * 1024 * 1024;
pub(crate) const MAX_REQUEST_PARAMS_JSON_BYTES: usize = 7 * 1024 * 1024;
pub(crate) const REQUEST_PARAMS_JSON_HEADROOM_BYTES: usize = 1024 * 1024;
pub(crate) const IPC_REQUEST_ENVELOPE_HEADROOM_BYTES: usize = 1024 * 1024;
const MAX_SHORT_TEXT_BYTES: usize = 16 * 1024;

pub trait ValidateRequest {
    fn validate(&self) -> Result<(), String>;
}

fn require_nonempty(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{field} is required"))
    } else {
        Ok(())
    }
}

fn require_max_bytes(value: &str, field: &str, maximum: usize) -> Result<(), String> {
    if value.len() > maximum {
        Err(format!("{field} exceeds the allowed size"))
    } else {
        Ok(())
    }
}

fn is_iso_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
    {
        return false;
    }
    let year = value[0..4].parse::<u32>().ok();
    let month = value[5..7].parse::<u32>().ok();
    let day = value[8..10].parse::<u32>().ok();
    let (Some(year), Some(month), Some(day)) = (year, month, day) else {
        return false;
    };
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    day > 0 && day <= days
}

fn require_iso_date(value: &str, field: &str) -> Result<(), String> {
    if is_iso_date(value) {
        Ok(())
    } else {
        Err(format!("{field} must be an ISO calendar date"))
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecordLoadRequest {
    date: String,
}

impl ValidateRequest for RecordLoadRequest {
    fn validate(&self) -> Result<(), String> {
        require_iso_date(&self.date, "date")
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum TransactionType {
    Expense,
    Income,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RecordEvent {
    time: String,
    title: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Transaction {
    #[serde(rename = "type")]
    kind: TransactionType,
    category: String,
    amount: f64,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecordSaveRequest {
    date: String,
    events: Vec<RecordEvent>,
    transactions: Vec<Transaction>,
    diary: String,
}

impl ValidateRequest for RecordSaveRequest {
    fn validate(&self) -> Result<(), String> {
        require_iso_date(&self.date, "date")?;
        require_max_bytes(&self.diary, "diary", MAX_TEXT_BYTES)?;
        if self.events.len() > 10_000 || self.transactions.len() > 10_000 {
            return Err("record collection exceeds the allowed size".to_string());
        }
        for event in &self.events {
            require_max_bytes(&event.time, "event.time", 64)?;
            require_max_bytes(&event.title, "event.title", MAX_SHORT_TEXT_BYTES)?;
        }
        for transaction in &self.transactions {
            if !transaction.amount.is_finite() {
                return Err("transaction.amount must be finite".to_string());
            }
            require_max_bytes(
                &transaction.category,
                "transaction.category",
                MAX_SHORT_TEXT_BYTES,
            )?;
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsultMode {
    Consult,
    InterviewSim,
    EsReview,
    GdSim,
    RomanceAnalysis,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Persona {
    name: String,
    r#trait: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum InterviewDifficulty {
    Standard,
    Hard,
    Extreme,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum InterviewStance {
    Adversarial,
    Standard,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct InterviewConfig {
    industry: String,
    genre: String,
    difficulty: InterviewDifficulty,
    stance: InterviewStance,
    #[serde(rename = "customTheme", default, skip_serializing_if = "Option::is_none")]
    custom_theme: Option<String>,
    /// M20-N: 企業別 ES の id。空文字 = ゼロベース面接。
    #[serde(rename = "esId", default, skip_serializing_if = "Option::is_none")]
    es_id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConsultRequest {
    query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mode: Option<ConsultMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    personas: Option<Vec<Persona>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    response_time_sec: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    config: Option<InterviewConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    external_research_id: Option<String>,
}

impl ValidateRequest for ConsultRequest {
    fn validate(&self) -> Result<(), String> {
        require_nonempty(&self.query, "query")?;
        require_max_bytes(&self.query, "query", MAX_TEXT_BYTES)?;
        if let Some(value) = self.response_time_sec {
            if !value.is_finite() || value < 0.0 {
                return Err("response_time_sec must be a non-negative finite number".to_string());
            }
        }
        if let Some(personas) = &self.personas {
            if personas.len() > 9 {
                return Err("personas exceeds the allowed count".to_string());
            }
            for persona in personas {
                require_nonempty(&persona.name, "persona.name")?;
                require_max_bytes(&persona.name, "persona.name", 512)?;
                require_max_bytes(&persona.r#trait, "persona.trait", MAX_SHORT_TEXT_BYTES)?;
            }
        }
        if let Some(config) = &self.config {
            require_max_bytes(&config.industry, "config.industry", MAX_SHORT_TEXT_BYTES)?;
            require_max_bytes(&config.genre, "config.genre", MAX_SHORT_TEXT_BYTES)?;
            if let Some(theme) = &config.custom_theme {
                require_max_bytes(theme, "config.customTheme", MAX_SHORT_TEXT_BYTES)?;
            }
            if let Some(es_id) = &config.es_id {
                require_max_bytes(es_id, "config.esId", MAX_SHORT_TEXT_BYTES)?;
            }
        }
        if let Some(ext_id) = &self.external_research_id {
            require_nonempty(ext_id, "external_research_id")?;
            if ext_id.len() != 64 || !ext_id.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) {
                return Err("external_research_id must be 64 lowercase hex".to_string());
            }
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CalendarMergeMode {
    Append,
    Overwrite,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ImportedFile {
    content: String,
    filename: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CalendarIcsRequest {
    mode: CalendarMergeMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ics_content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ics_files: Option<Vec<ImportedFile>>,
}

impl ValidateRequest for CalendarIcsRequest {
    fn validate(&self) -> Result<(), String> {
        match (&self.ics_content, &self.ics_files) {
            (Some(content), None) => {
                require_nonempty(content, "ics_content")?;
                require_max_bytes(content, "ics_content", MAX_TEXT_BYTES)
            }
            (None, Some(files)) if !files.is_empty() => {
                if files.len() > 128 {
                    return Err("ics_files exceeds the allowed count".to_string());
                }
                for file in files {
                    require_nonempty(&file.content, "ics_files.content")?;
                    require_max_bytes(&file.content, "ics_files.content", MAX_TEXT_BYTES)?;
                    require_max_bytes(&file.filename, "ics_files.filename", MAX_SHORT_TEXT_BYTES)?;
                }
                Ok(())
            }
            _ => Err("exactly one ICS payload is required".to_string()),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CalendarAppleRequest {
    mode: CalendarMergeMode,
}

impl ValidateRequest for CalendarAppleRequest {
    fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImportLineSingleRequest {
    content: String,
    filename: String,
}

impl ValidateRequest for ImportLineSingleRequest {
    fn validate(&self) -> Result<(), String> {
        require_nonempty(&self.content, "content")?;
        require_max_bytes(&self.content, "content", MAX_TEXT_BYTES)?;
        require_max_bytes(&self.filename, "filename", MAX_SHORT_TEXT_BYTES)
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImportLineBatchRequest {
    files: Vec<ImportedFile>,
}

impl ValidateRequest for ImportLineBatchRequest {
    fn validate(&self) -> Result<(), String> {
        if self.files.is_empty() || self.files.len() > 128 {
            return Err("files must contain between 1 and 128 entries".to_string());
        }
        for file in &self.files {
            require_nonempty(&file.content, "files.content")?;
            require_max_bytes(&file.content, "files.content", MAX_TEXT_BYTES)?;
            require_max_bytes(&file.filename, "files.filename", MAX_SHORT_TEXT_BYTES)?;
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImportClassifyRequest {
    content: String,
    filename: String,
}

impl ValidateRequest for ImportClassifyRequest {
    fn validate(&self) -> Result<(), String> {
        require_max_bytes(&self.content, "content", MAX_TEXT_BYTES)?;
        require_max_bytes(&self.filename, "filename", MAX_SHORT_TEXT_BYTES)
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportDestination {
    Es,
    Knowledge,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImportDocumentRequest {
    content: String,
    filename: String,
    dest: ImportDestination,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    company_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    confirm_overwrite: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    replace_es_id: Option<String>,
}

impl ValidateRequest for ImportDocumentRequest {
    fn validate(&self) -> Result<(), String> {
        require_nonempty(&self.content, "content")?;
        require_max_bytes(&self.content, "content", MAX_TEXT_BYTES)?;
        require_max_bytes(&self.filename, "filename", MAX_SHORT_TEXT_BYTES)?;
        if let Some(company) = &self.company_name {
            require_max_bytes(company, "company_name", MAX_SHORT_TEXT_BYTES)?;
        }
        if let Some(replace_id) = &self.replace_es_id {
            require_max_bytes(replace_id, "replace_es_id", MAX_SHORT_TEXT_BYTES)?;
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LlmWarmRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    probe_llm: Option<bool>,
}

impl ValidateRequest for LlmWarmRequest {
    fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsSaveFixedRequest {
    attributes: BTreeMap<String, String>,
}

impl ValidateRequest for SettingsSaveFixedRequest {
    fn validate(&self) -> Result<(), String> {
        if self.attributes.len() > 256 {
            return Err("attributes exceeds the allowed count".to_string());
        }
        for (key, value) in &self.attributes {
            require_nonempty(key, "attributes.key")?;
            require_max_bytes(key, "attributes.key", 512)?;
            require_max_bytes(value, "attributes.value", MAX_SHORT_TEXT_BYTES)?;
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NarrativeCompileRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    target_domain: Option<String>,
}

impl ValidateRequest for NarrativeCompileRequest {
    fn validate(&self) -> Result<(), String> {
        if let Some(domain) = &self.target_domain {
            require_max_bytes(domain, "target_domain", MAX_SHORT_TEXT_BYTES)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Scope {
    Global,
    Dyad,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeRequest {
    scope: Scope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    alias: Option<String>,
}

impl ValidateRequest for ScopeRequest {
    fn validate(&self) -> Result<(), String> {
        match (&self.scope, &self.alias) {
            (Scope::Global, None) => Ok(()),
            (Scope::Dyad, Some(alias)) => {
                require_nonempty(alias, "alias")?;
                require_max_bytes(alias, "alias", 512)
            }
            _ => Err("scope and alias are inconsistent".to_string()),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum TwinMode {
    Daily,
    Interview,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TwinCalendarItem {
    date: String,
    time: String,
    title: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TwinScenario {
    horizon_days: u32,
    calendar: Vec<TwinCalendarItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mode: Option<TwinMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    interview_turns: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TwinForecastRequest {
    scenario: TwinScenario,
    scope: Scope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    alias: Option<String>,
}

impl ValidateRequest for TwinForecastRequest {
    fn validate(&self) -> Result<(), String> {
        if !(1..=60).contains(&self.scenario.horizon_days) {
            return Err("scenario.horizon_days is outside 1..=60".to_string());
        }
        if self.scenario.calendar.len() > 10_000 {
            return Err("scenario.calendar exceeds the allowed count".to_string());
        }
        for item in &self.scenario.calendar {
            require_iso_date(&item.date, "scenario.calendar.date")?;
            require_max_bytes(&item.time, "scenario.calendar.time", 64)?;
            require_max_bytes(&item.title, "scenario.calendar.title", MAX_SHORT_TEXT_BYTES)?;
        }
        if let Some(turns) = self.scenario.interview_turns {
            if turns > 20 {
                return Err("scenario.interview_turns is outside the allowed range".to_string());
            }
        }
        ScopeRequest {
            scope: match self.scope {
                Scope::Global => Scope::Global,
                Scope::Dyad => Scope::Dyad,
            },
            alias: self.alias.clone(),
        }
        .validate()
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeDateRequest {
    today: String,
}

impl ValidateRequest for ProbeDateRequest {
    fn validate(&self) -> Result<(), String> {
        require_iso_date(&self.today, "today")
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeAnswerRequest {
    session_id: String,
    question_id: String,
    answer: String,
    today: String,
}

impl ValidateRequest for ProbeAnswerRequest {
    fn validate(&self) -> Result<(), String> {
        require_nonempty(&self.session_id, "session_id")?;
        require_nonempty(&self.question_id, "question_id")?;
        require_nonempty(&self.answer, "answer")?;
        require_max_bytes(&self.session_id, "session_id", 512)?;
        require_max_bytes(&self.question_id, "question_id", 512)?;
        require_max_bytes(&self.answer, "answer", MAX_TEXT_BYTES)?;
        require_iso_date(&self.today, "today")
    }
}

/// STEP 6: explicit external research request (no URL / no live egress by default).
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeResearchRequest {
    query: String,
}

impl ValidateRequest for KnowledgeResearchRequest {
    fn validate(&self) -> Result<(), String> {
        require_nonempty(&self.query, "query")?;
        require_max_bytes(&self.query, "query", MAX_SHORT_TEXT_BYTES)
    }
}

/// STEP 8: user consent toggle for external research.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgePolicySetRequest {
    pub enabled: bool,
}

impl ValidateRequest for KnowledgePolicySetRequest {
    fn validate(&self) -> Result<(), String> {
        let _ = self.enabled;
        Ok(())
    }
}

/// Optional ES id for `es.view` (empty / absent = latest active ES).
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EsViewRequest {
    #[serde(default)]
    pub id: Option<String>,
}

impl ValidateRequest for EsViewRequest {
    fn validate(&self) -> Result<(), String> {
        if let Some(ref id) = self.id {
            require_max_bytes(id, "id", 512)?;
        }
        Ok(())
    }
}
