//! Apple EventKit calendar read bridge (M20 データ連携 Part 2).
//!
//! Offline / on-device only — no network, no write access (read-only:
//! `eventsMatchingPredicate:`, never `saveEvent:span:error:`). Mirrors the
//! `objc2-vision` binding style in `ocr/vision.rs`: unsafe confined to this
//! file, safe Rust everywhere else.
//!
//! Vault injection (turning these events into RAG-searchable / CONSULT
//! context, the way `rag::line_import` does for LINE) is a deliberate
//! follow-up — this module currently returns plain structured event data.

use std::sync::mpsc;
use std::time::Duration;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2_event_kit::{EKAuthorizationStatus, EKEntityType, EKEvent, EKEventStore};
use objc2_foundation::{NSDate, NSError};

/// EventKit dispatches the completion block on an internal system queue
/// regardless of whether the calling thread runs a run loop (unlike older
/// run-loop-bound APIs), so a bounded wait is a safety net against an
/// unexpected OS-side hang, not the expected path.
const ACCESS_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Hard cap on events returned per call (Jetsam / IPC payload bound, mirrors
/// `rag::chunk::MAX_CHUNKS` / `context_merger::MAX_EVENTS` conventions).
pub const MAX_EVENTS: usize = 500;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct CalendarEventOut {
    pub id: Option<String>,
    pub title: String,
    /// Unix seconds (UTC).
    pub start: f64,
    /// Unix seconds (UTC).
    pub end: f64,
    pub all_day: bool,
    pub calendar_title: Option<String>,
    pub notes: Option<String>,
}

/// Construct a fresh event store. Cheap; EventKit caches the underlying
/// database connection internally, so callers may create one per request.
pub fn new_event_store() -> Retained<EKEventStore> {
    // SAFETY: `+[EKEventStore new]` is the generated Objective-C constructor.
    // It returns a retained, typed owner and requires no caller-provided
    // pointers or lifetime extension (same pattern as `LAContext::new()` in
    // `db/secure_vault.rs`).
    unsafe { EKEventStore::new() }
}

fn authorization_status(store: &EKEventStore) -> EKAuthorizationStatus {
    // SAFETY: `authorizationStatusForEntityType:` is a class-side query with
    // no side effects; `EKEntityType::Event` is a documented, valid constant.
    // `store` is only used to select the class via `EKEventStore::` — the
    // Objective-C method is declared `+ (EKAuthorizationStatus)...`, i.e. it
    // does not read `self`, matching every other objc2 binding for it.
    let _ = store;
    unsafe { EKEventStore::authorizationStatusForEntityType(EKEntityType::Event) }
}

/// Request full calendar (event) access, blocking until the user decides or
/// the request times out. iOS only prompts once per app install — repeat
/// calls after a decision has been made return immediately without a dialog.
///
/// Never assumes access was granted: callers must check the returned status
/// (`EKAuthorizationStatus::FullAccess`) before calling [`fetch_events`].
pub fn request_full_access(store: &EKEventStore) -> Result<EKAuthorizationStatus, String> {
    let current = authorization_status(store);
    if current != EKAuthorizationStatus::NotDetermined {
        return Ok(current);
    }

    let (tx, rx) = mpsc::sync_channel::<()>(1);
    let block: RcBlock<dyn Fn(Bool, *mut NSError)> =
        RcBlock::new(move |_granted: Bool, _error: *mut NSError| {
            // Re-query the authoritative status after the callback fires
            // rather than trusting the callback's own `granted` bool in
            // isolation — matches "single source of truth" style used
            // elsewhere in this codebase (e.g. vault status reads).
            let _ = tx.send(());
        });

    // SAFETY: `block` is a heap-allocated `RcBlock` kept alive on this stack
    // frame until `rx.recv_timeout` returns below, which is required before
    // EventKit invokes it. The raw pointer cast matches the exact type alias
    // `EKEventStoreRequestAccessCompletionHandler = *mut DynBlock<dyn Fn(Bool,
    // *mut NSError)>` generated for this method.
    unsafe {
        store.requestFullAccessToEventsWithCompletion(
            &*block as *const block2::Block<_> as *mut _,
        );
    }

    match rx.recv_timeout(ACCESS_REQUEST_TIMEOUT) {
        Ok(()) => Ok(authorization_status(store)),
        Err(_) => Err("calendar access request timed out".into()),
    }
}

/// Read events in `[start, end)` (Unix seconds, UTC) across all calendars.
///
/// Caller must have already confirmed `FullAccess` via
/// [`request_full_access`] / [`authorization_status`] — EventKit simply
/// returns an empty array (not an error) when access is missing, so an
/// upstream check is the only way to distinguish "no events" from "no
/// permission" for the user.
pub fn fetch_events(
    store: &EKEventStore,
    start_unix: f64,
    end_unix: f64,
) -> Result<Vec<CalendarEventOut>, String> {
    if !start_unix.is_finite() || !end_unix.is_finite() || end_unix <= start_unix {
        return Err("invalid date range".into());
    }

    // `dateWithTimeIntervalSince1970:` is a safe (non-`unsafe fn`) Foundation
    // constructor; both dates are valid, non-null `Retained` owners.
    let start_date = NSDate::dateWithTimeIntervalSince1970(start_unix);
    let end_date = NSDate::dateWithTimeIntervalSince1970(end_unix);

    // SAFETY: `predicateForEventsWithStartDate:endDate:calendars:` requires
    // only valid `NSDate` references and an optional calendars array; `None`
    // (nil) means "search all calendars", a documented, valid input.
    let predicate = unsafe {
        store.predicateForEventsWithStartDate_endDate_calendars(&start_date, &end_date, None)
    };

    // SAFETY: `predicate` was created by the matching `predicateFor...`
    // factory immediately above, satisfying the method's documented
    // precondition ("if this predicate was not created with the predicate
    // creation functions in this class, an exception is raised").
    let events = unsafe { store.eventsMatchingPredicate(&predicate) };

    let mut out = Vec::with_capacity(events.len().min(MAX_EVENTS));
    for event in events.iter().take(MAX_EVENTS) {
        out.push(to_calendar_event_out(&event));
    }
    Ok(out)
}

fn to_calendar_event_out(event: &EKEvent) -> CalendarEventOut {
    // SAFETY: all accessors below are plain Objective-C property getters on a
    // live `EKEvent` we just received from `eventsMatchingPredicate:`; none
    // take caller-supplied pointers.
    unsafe {
        let id = event.eventIdentifier().map(|s| s.to_string());
        let title = event.title().to_string();
        let start = event.startDate().timeIntervalSince1970();
        let end = event.endDate().timeIntervalSince1970();
        let all_day = event.isAllDay();
        let calendar_title = event.calendar().map(|c| c.title().to_string());
        let notes = event.notes().map(|s| s.to_string());
        CalendarEventOut {
            id,
            title,
            start,
            end,
            all_day,
            calendar_title,
            notes,
        }
    }
}

// Keep ClassType live (Zero Warnings across feature matrices) — matches
// `ocr/vision.rs`'s convention of a single anchor per bridged Apple class.
const _: fn() -> &'static objc2::runtime::AnyClass = {
    use objc2::ClassType;
    EKEventStore::class
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetch_events_rejects_inverted_or_non_finite_range() {
        let store = new_event_store();
        assert!(fetch_events(&store, f64::NAN, 100.0).is_err());
        assert!(fetch_events(&store, 100.0, f64::INFINITY).is_err());
        assert!(fetch_events(&store, 100.0, 100.0).is_err(), "end must be strictly after start");
        assert!(fetch_events(&store, 200.0, 100.0).is_err(), "end before start rejected");
    }
}
