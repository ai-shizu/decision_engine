//! Tauri command surface for the EventKit calendar read bridge (M20 Part 2).
//!
//! Read-only, on-device, no network. FE persists via `sync_daily_context`
//! after a successful fetch (ImportTab EventKit block).

use objc2_event_kit::EKAuthorizationStatus;
use serde::{Deserialize, Serialize};

use super::event_kit::{self, CalendarEventOut};

/// Sane upper bound on a single query span, independent of EventKit's own
/// (~4 year) internal cap — avoids an accidental multi-year backlog request
/// from a frontend bug turning into a very large event scan.
const MAX_RANGE_SECS: f64 = 400.0 * 24.0 * 3600.0;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchAppleCalendarEventsParams {
    /// Unix seconds (UTC), inclusive start of range.
    pub start_unix: f64,
    /// Unix seconds (UTC), exclusive end of range.
    pub end_unix: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct FetchAppleCalendarEventsResult {
    pub authorized: bool,
    pub status: String,
    pub events: Vec<CalendarEventOut>,
}

fn status_label(status: EKAuthorizationStatus) -> &'static str {
    match status {
        EKAuthorizationStatus::NotDetermined => "not_determined",
        EKAuthorizationStatus::Restricted => "restricted",
        EKAuthorizationStatus::Denied => "denied",
        EKAuthorizationStatus::FullAccess => "full_access",
        EKAuthorizationStatus::WriteOnly => "write_only",
        _ => "unknown",
    }
}

/// Request calendar access (prompts at most once) and read events in range.
/// When access is anything other than `full_access`, returns an empty event
/// list with `authorized: false` rather than an error — matching the
/// fail-safe / soft-unavailable convention used by `llm::consult_context`
/// (never a hard error for a permission state the user can still act on).
#[tauri::command]
pub async fn fetch_apple_calendar_events(
    params: FetchAppleCalendarEventsParams,
) -> Result<FetchAppleCalendarEventsResult, String> {
    if !params.start_unix.is_finite()
        || !params.end_unix.is_finite()
        || params.end_unix <= params.start_unix
    {
        return Err("invalid date range".into());
    }
    if params.end_unix - params.start_unix > MAX_RANGE_SECS {
        return Err("date range too large".into());
    }

    tauri::async_runtime::spawn_blocking(move || {
        let store = event_kit::new_event_store();
        let status = event_kit::request_full_access(&store)?;
        if status != EKAuthorizationStatus::FullAccess {
            return Ok(FetchAppleCalendarEventsResult {
                authorized: false,
                status: status_label(status).to_string(),
                events: Vec::new(),
            });
        }
        let events = event_kit::fetch_events(&store, params.start_unix, params.end_unix)?;
        Ok(FetchAppleCalendarEventsResult {
            authorized: true,
            status: status_label(status).to_string(),
            events,
        })
    })
    .await
    .map_err(|_| "calendar fetch task join failed".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_label_covers_all_known_statuses() {
        assert_eq!(status_label(EKAuthorizationStatus::NotDetermined), "not_determined");
        assert_eq!(status_label(EKAuthorizationStatus::Restricted), "restricted");
        assert_eq!(status_label(EKAuthorizationStatus::Denied), "denied");
        assert_eq!(status_label(EKAuthorizationStatus::FullAccess), "full_access");
        assert_eq!(status_label(EKAuthorizationStatus::WriteOnly), "write_only");
    }
}
