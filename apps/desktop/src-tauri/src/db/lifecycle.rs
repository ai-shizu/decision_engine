//! iOS background auto-lock (docs/m3_action_plan.md §0, §8 Lifecycle row).
//!
//! When the app enters the background or protected data is about to become
//! unavailable, the encrypted connection must be closed and the in-memory key
//! zeroized. `VaultHandle::lock()` already performs that; this module only
//! wires the iOS lifecycle triggers to it.
//!
//! Trigger choice: Tauri's public `RunEvent` exposes `Resumed` (foreground)
//! but no "entered background" variant, so it cannot drive a background lock.
//! We therefore observe the first-party UIKit notifications
//! `UIApplicationDidEnterBackgroundNotification` and
//! `UIApplicationProtectedDataWillBecomeUnavailable`, whose delivery is
//! guaranteed by the OS. `WillResignActive` is deliberately NOT observed: a
//! notification-center pull-down must not force a re-authentication ceremony
//! (§1.1, one OS ceremony per foreground).

use std::{thread, time::Duration};

use objc2::{
    define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, NSObject, NSObjectProtocol},
    sel, AnyThread, DefinedClass,
};
use objc2_foundation::NSNotificationCenter;
use objc2_ui_kit::{
    UIApplicationDidEnterBackgroundNotification, UIApplicationProtectedDataWillBecomeUnavailable,
};

use super::{VaultErrorCode, VaultHandle};

/// Bounded lock retries. A backgrounded iOS app has only a short window before
/// suspension, so we never spin unboundedly. If every attempt fails, iOS
/// process suspension plus the `WhenPasscodeSetThisDeviceOnly` Data Protection
/// class remain the last line of defense — this trigger is a best-effort
/// tightening, not the sole guarantee.
const AUTO_LOCK_MAX_ATTEMPTS: u8 = 3;
const AUTO_LOCK_RETRY_DELAY: Duration = Duration::from_millis(100);

struct ObserverIvars {
    vault: VaultHandle,
}

define_class!(
    // SAFETY:
    // - The superclass NSObject has no subclassing requirements.
    // - This class does not implement `Drop`.
    #[unsafe(super(NSObject))]
    #[name = "PkbVaultLifecycleObserver"]
    #[ivars = ObserverIvars]
    struct LifecycleObserver;

    impl LifecycleObserver {
        // Invoked on the main thread by NSNotificationCenter. The notification
        // payload is intentionally ignored (never read).
        #[unsafe(method(onVaultLifecycleEvent:))]
        fn on_vault_lifecycle_event(&self, _notification: *mut AnyObject) {
            let vault = self.ivars().vault.clone();
            // Lock off the main thread so UIKit's background-transition handler
            // is never blocked by the worker round trip.
            let _ = thread::Builder::new()
                .name("pkb-vault-autolock".to_string())
                .spawn(move || run_bounded_lock(&vault));
        }
    }

    unsafe impl NSObjectProtocol for LifecycleObserver {}
);

fn run_bounded_lock(vault: &VaultHandle) {
    for attempt in 0..AUTO_LOCK_MAX_ATTEMPTS {
        match vault.lock() {
            // Locked (idempotent even if already locked) or no worker at all.
            Ok(_) | Err(VaultErrorCode::Unavailable) => return,
            // Transient contention: retry within the background window.
            Err(VaultErrorCode::Busy) | Err(VaultErrorCode::Timeout) => {
                if attempt + 1 < AUTO_LOCK_MAX_ATTEMPTS {
                    thread::sleep(AUTO_LOCK_RETRY_DELAY);
                }
            }
            // Any other code is not made safer by retrying.
            Err(_) => return,
        }
    }
}

/// Register the background / protected-data lifecycle observers for `vault`.
///
/// Must be called on the main thread (the Tauri `setup` closure is). The
/// observer is intentionally leaked: `addObserver:selector:name:object:` keeps
/// only an unretained reference, so the observer must live for the whole
/// process. It is never unregistered because it dies with the process.
pub(crate) fn install_auto_lock(vault: VaultHandle) {
    let this = LifecycleObserver::alloc().set_ivars(ObserverIvars { vault });
    // SAFETY: `init` on our NSObject subclass; `set_ivars` was called first.
    let observer: Retained<LifecycleObserver> = unsafe { msg_send![super(this), init] };

    let center = NSNotificationCenter::defaultCenter();

    // SAFETY: `observer` is a live instance of our class exposing the
    // `onVaultLifecycleEvent:` selector; both names are framework-owned
    // immutable NSStrings; no source-object filter is used.
    unsafe {
        center.addObserver_selector_name_object(
            &*observer,
            sel!(onVaultLifecycleEvent:),
            Some(UIApplicationDidEnterBackgroundNotification),
            None,
        );
        center.addObserver_selector_name_object(
            &*observer,
            sel!(onVaultLifecycleEvent:),
            Some(UIApplicationProtectedDataWillBecomeUnavailable),
            None,
        );
    }

    // Keep the observer alive for the process lifetime (see doc comment).
    std::mem::forget(observer);
}
