//! Phase 10 — Taptic Engine feedback via UIKit (offline, no npm plugin).
//!
//! Official `@tauri-apps/plugin-haptics` is equivalent in spirit; this crate
//! keeps the existing pinned `objc2-ui-kit` stack and never calls
//! `navigator.vibrate`. Non-iOS builds soft-no-op so the FE invoke is safe.

use tauri::AppHandle;

/// FE kinds: `selection` | `impact` | `impact_heavy` | `warning`.
#[tauri::command]
pub async fn haptic_feedback(app: AppHandle, kind: String) -> Result<(), String> {
    #[cfg(all(feature = "secure-vault", target_os = "ios"))]
    {
        let (tx, rx) = std::sync::mpsc::channel();
        app.run_on_main_thread(move || {
            let _ = tx.send(ios::fire(&kind));
        })
        .map_err(|e| format!("haptic main-thread schedule failed: {e}"))?;
        return rx
            .recv()
            .map_err(|_| "haptic main-thread reply dropped".to_string())?;
    }
    #[cfg(not(all(feature = "secure-vault", target_os = "ios")))]
    {
        let _ = (app, kind);
        Ok(())
    }
}

#[cfg(all(feature = "secure-vault", target_os = "ios"))]
mod ios {
    use objc2::MainThreadMarker;
    use objc2::MainThreadOnly;
    use objc2_ui_kit::{
        UIImpactFeedbackGenerator, UIImpactFeedbackStyle, UINotificationFeedbackGenerator,
        UINotificationFeedbackType, UISelectionFeedbackGenerator,
    };

    pub(super) fn fire(kind: &str) -> Result<(), String> {
        let Some(mtm) = MainThreadMarker::new() else {
            return Err("haptic requires main thread".into());
        };
        match kind {
            "selection" => {
                let g = UISelectionFeedbackGenerator::new(mtm);
                g.prepare();
                g.selectionChanged();
            }
            "impact" | "impact_medium" => {
                let g = UIImpactFeedbackGenerator::initWithStyle(
                    UIImpactFeedbackGenerator::alloc(mtm),
                    UIImpactFeedbackStyle::Medium,
                );
                g.prepare();
                g.impactOccurred();
            }
            "impact_heavy" | "warning_heavy" => {
                let g = UIImpactFeedbackGenerator::initWithStyle(
                    UIImpactFeedbackGenerator::alloc(mtm),
                    UIImpactFeedbackStyle::Heavy,
                );
                g.prepare();
                g.impactOccurred();
            }
            "warning" => {
                let g = UINotificationFeedbackGenerator::new(mtm);
                g.prepare();
                g.notificationOccurred(UINotificationFeedbackType::Warning);
            }
            // Unknown kinds: soft no-op (never fail consult / probe UX).
            _ => {}
        }
        Ok(())
    }
}
