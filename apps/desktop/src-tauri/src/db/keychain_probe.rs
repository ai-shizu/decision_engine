//! M3 Phase 0 / §4.2.1 typed Keychain ownership and final-link probe.
//!
//! This module deliberately stops before Keychain item I/O. It constructs the
//! approved access-control and authentication-context objects, builds a typed
//! heterogeneous query, and keeps the SecItem entry point reachable for the
//! native linker without calling it.

use core::{ffi::c_void, ptr, ptr::NonNull};
use std::hint::black_box;

use objc2::{rc::Retained, runtime::AnyObject};
use objc2_core_foundation::{CFDictionary, CFMutableDictionary, CFType};
use objc2_foundation::{NSMutableDictionary, NSNumber, NSString};
use objc2_local_authentication::LAContext;
use objc2_security::{
    kSecAttrAccessibleWhenPasscodeSetThisDeviceOnly, kSecAttrService, kSecClass,
    kSecClassGenericPassword, kSecRandomDefault, kSecReturnData, kSecUseAuthenticationContext,
    SecAccessControl, SecAccessControlCreateFlags, SecItemCopyMatching, SecRandomCopyBytes,
};
use zeroize::Zeroizing;

const LOCALIZED_REASON: &str = "暗号化されたデータのロックを解除します。";
const PROBE_SERVICE: &str = "pkb.m3.phase0.typed-keychain-link-probe";
const ERR_SECRANDOM: &str = "secure_vault_link_probe: SecRandom failed";
const ERR_ACCESS_CONTROL: &str = "secure_vault_link_probe: SecAccessControl failed";

type SecItemCopyMatchingFn = unsafe extern "C-unwind" fn(&CFDictionary, *mut *const CFType) -> i32;

/// Owns the original authentication context and the independently retaining
/// Foundation query. No raw ownership conversion is involved.
struct TypedKeychainQuery {
    context: Retained<LAContext>,
    query: Retained<NSMutableDictionary<NSString, AnyObject>>,
}

impl TypedKeychainQuery {
    fn build() -> Self {
        // SAFETY: This is the generated LocalAuthentication constructor. Its
        // +1 result is immediately owned by Retained and released by Drop.
        let context = unsafe { LAContext::new() };
        let reason = NSString::from_str(LOCALIZED_REASON);
        // SAFETY: This generated property setter copies the NSString. Both
        // objects are valid Objective-C instances for the duration of the call.
        unsafe { context.setLocalizedReason(&reason) };

        let query = NSMutableDictionary::<NSString, AnyObject>::new();

        // SAFETY: These are immutable framework-exported CFString constants.
        // The typed toll-free AsRef conversions borrow them; insert copies each
        // key and retains each value, including the same LAContext instance.
        unsafe {
            let class_key: &NSString = kSecClass.as_ref();
            let class_value: &AnyObject = kSecClassGenericPassword.as_ref();
            query.insert(class_key, class_value);

            let service_key: &NSString = kSecAttrService.as_ref();
            let service = NSString::from_str(PROBE_SERVICE);
            let service_value: &AnyObject = service.as_ref();
            query.insert(service_key, service_value);

            let return_data_key: &NSString = kSecReturnData.as_ref();
            let return_data = NSNumber::new_bool(true);
            let return_data_value: &AnyObject = return_data.as_ref();
            query.insert(return_data_key, return_data_value);

            let auth_context_key: &NSString = kSecUseAuthenticationContext.as_ref();
            let auth_context_value: &AnyObject = context.as_ref();
            query.insert(auth_context_key, auth_context_value);
        }

        Self { context, query }
    }

    fn as_cf_dictionary(&self) -> &CFDictionary {
        let mutable: &CFMutableDictionary<NSString, AnyObject> = self.query.as_ref();
        mutable.as_opaque()
    }
}

/// Exercise constructors and preserve final-link reachability without reading,
/// writing, updating, or deleting any Keychain item.
pub(super) fn verify_typed_keychain_link_probe() -> Result<(), String> {
    let mut key = Zeroizing::new([0u8; 32]);
    let output = NonNull::from(&mut key[0]).cast::<c_void>();

    // SAFETY: kSecRandomDefault is the supported immutable generator handle;
    // output points to the first byte of a live 32-byte writable allocation.
    let random_status = unsafe { SecRandomCopyBytes(kSecRandomDefault, key.len(), output) };
    if random_status != 0 {
        return Err(ERR_SECRANDOM.to_string());
    }

    // SAFETY: The exported protection constant is a CFString/CFType of the
    // required kind, the error output is explicitly allowed to be null, and
    // the returned Create-rule object is owned by CFRetained.
    let access_control = unsafe {
        let protection = kSecAttrAccessibleWhenPasscodeSetThisDeviceOnly;
        SecAccessControl::with_flags(
            None,
            protection.as_ref(),
            SecAccessControlCreateFlags::UserPresence,
            ptr::null_mut(),
        )
    }
    .ok_or_else(|| ERR_ACCESS_CONTROL.to_string())?;

    let probe = TypedKeychainQuery::build();
    let typed_query = probe.as_cf_dictionary();

    // Coercing the generated function item to its exact function-pointer type
    // creates final-link reachability. It is never called, so this probe cannot
    // trigger a Keychain search or authentication UI.
    let copy_matching: SecItemCopyMatchingFn = SecItemCopyMatching;
    black_box((typed_query, copy_matching, &probe.context, &access_control));

    Ok(())
}
