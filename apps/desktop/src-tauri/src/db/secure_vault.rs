//! M3 Phase 1-A production Keychain boundary for the SQLCipher master key.
//!
//! This module owns Keychain retrieval and first-run key generation only.
//! Opening SQLite and applying the key are deliberately deferred to Phase 1-B.

use core::{ffi::c_void, ptr, ptr::NonNull};
use std::{error::Error, fmt};

use objc2::{rc::Retained, runtime::AnyObject};
use objc2_core_foundation::{
    kCFAllocatorNull, CFData, CFDictionary, CFIndex, CFMutableDictionary, CFRetained, CFType,
};
use objc2_foundation::{NSMutableDictionary, NSNumber, NSString};
use objc2_local_authentication::LAContext;
use objc2_security::{
    errSecAuthFailed, errSecDuplicateItem, errSecInteractionNotAllowed, errSecItemNotFound,
    errSecSuccess, errSecUserCanceled, kSecAttrAccessControl,
    kSecAttrAccessibleWhenPasscodeSetThisDeviceOnly, kSecAttrAccount, kSecAttrService,
    kSecAttrSynchronizable, kSecClass, kSecClassGenericPassword, kSecMatchLimit, kSecMatchLimitOne,
    kSecRandomDefault, kSecReturnData, kSecUseAuthenticationContext, kSecValueData,
    SecAccessControl, SecAccessControlCreateFlags, SecItemAdd, SecItemCopyMatching,
    SecRandomCopyBytes,
};
use zeroize::Zeroizing;

const KEY_LENGTH_BYTES: usize = 32;
const KEYCHAIN_SERVICE: &str = "com.ai-shizu.pkb.secure-vault";
const KEYCHAIN_ACCOUNT: &str = "sqlcipher-master-key-v1";
const LOCALIZED_REASON: &str = "暗号化されたデータのロックを解除します。";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeychainOperation {
    Retrieve,
    Add,
}

impl fmt::Display for KeychainOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Retrieve => formatter.write_str("retrieve"),
            Self::Add => formatter.write_str("add"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SecureVaultError {
    RandomGenerationFailed {
        status: i32,
    },
    AccessControlCreationFailed,
    KeyDataViewCreationFailed,
    AuthenticationCancelled,
    AuthenticationFailed,
    InteractionNotAllowed,
    KeychainStatus {
        operation: KeychainOperation,
        status: i32,
    },
    MissingKeychainResult,
    UnexpectedKeychainResultType,
    InvalidKeyLength {
        actual: usize,
    },
    DuplicateItemRace,
}

impl fmt::Display for SecureVaultError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RandomGenerationFailed { status } => {
                write!(
                    formatter,
                    "secure random generation failed with status {status}"
                )
            }
            Self::AccessControlCreationFailed => {
                formatter.write_str("Keychain access-control creation failed")
            }
            Self::KeyDataViewCreationFailed => {
                formatter.write_str("Keychain key-data view creation failed")
            }
            Self::AuthenticationCancelled => formatter.write_str("authentication was cancelled"),
            Self::AuthenticationFailed => formatter.write_str("authentication failed"),
            Self::InteractionNotAllowed => {
                formatter.write_str("Keychain authentication interaction is not allowed")
            }
            Self::KeychainStatus { operation, status } => {
                write!(
                    formatter,
                    "Keychain {operation} failed with status {status}"
                )
            }
            Self::MissingKeychainResult => {
                formatter.write_str("Keychain returned success without key data")
            }
            Self::UnexpectedKeychainResultType => {
                formatter.write_str("Keychain returned a non-data result")
            }
            Self::InvalidKeyLength { actual } => {
                write!(
                    formatter,
                    "Keychain returned an invalid key length: {actual}"
                )
            }
            Self::DuplicateItemRace => formatter
                .write_str("Keychain item appeared during creation but could not be retrieved"),
        }
    }
}

impl Error for SecureVaultError {}

enum LookupOutcome {
    Found(Zeroizing<Vec<u8>>),
    NotFound,
}

/// Keychain-backed owner of the 256-bit SQLCipher master key.
///
/// The returned plaintext is always held by `Zeroizing<Vec<u8>>`. This type
/// intentionally does not own a SQLite connection or persist key bytes outside
/// the Keychain.
pub struct SecureVault;

impl SecureVault {
    /// Retrieve the existing master key after user-presence authentication, or
    /// generate and persist a new key on first use.
    pub fn retrieve_or_generate_key(
        context: &Retained<LAContext>,
    ) -> Result<Zeroizing<Vec<u8>>, SecureVaultError> {
        set_localized_reason(context);

        match retrieve_key(context)? {
            LookupOutcome::Found(key) => Ok(key),
            LookupOutcome::NotFound => generate_and_store_key(context),
        }
    }
}

fn set_localized_reason(context: &Retained<LAContext>) {
    let reason = NSString::from_str(LOCALIZED_REASON);
    // SAFETY: This generated Objective-C property setter copies the NSString;
    // both typed objects remain valid throughout the synchronous call.
    unsafe { context.setLocalizedReason(&reason) };
}

fn retrieve_key(context: &Retained<LAContext>) -> Result<LookupOutcome, SecureVaultError> {
    let query = build_retrieval_query(context);
    let mut raw_result: *const CFType = ptr::null();

    // SAFETY: The query contains only documented Keychain key/value types. The
    // output pointer is valid and initialized to null. A successful `Copy`
    // result has +1 retain count under Core Foundation's Copy rule.
    let status = unsafe { SecItemCopyMatching(as_cf_dictionary(&query), &mut raw_result) };

    if status == errSecSuccess {
        key_from_copy_result(raw_result).map(LookupOutcome::Found)
    } else if status == errSecItemNotFound {
        Ok(LookupOutcome::NotFound)
    } else {
        Err(map_keychain_status(KeychainOperation::Retrieve, status))
    }
}

fn key_from_copy_result(raw_result: *const CFType) -> Result<Zeroizing<Vec<u8>>, SecureVaultError> {
    let result_ref = {
        // SAFETY: A successful SecItemCopyMatching call must return either a
        // valid CF object or null. This creates only a temporary borrow.
        unsafe { raw_result.as_ref() }.ok_or(SecureVaultError::MissingKeychainResult)?
    };

    // SAFETY: SecItemCopyMatching follows the Core Foundation Copy rule, so
    // the non-null result carries +1 retain count. CFRetained is the canonical
    // owner and releases it exactly once; no pointer cast or ownership bridge
    // for LAContext is involved.
    let result = unsafe { CFRetained::from_raw(NonNull::from(result_ref)) };
    let data = result
        .downcast::<CFData>()
        .map_err(|_| SecureVaultError::UnexpectedKeychainResultType)?;

    // Minimize the lifetime of the OS-owned plaintext buffer: immediately copy
    // into zeroizing Rust storage, then release CFData when this function exits.
    let key = Zeroizing::new(data.to_vec());
    validate_key_length(&key)?;
    Ok(key)
}

fn generate_and_store_key(
    context: &Retained<LAContext>,
) -> Result<Zeroizing<Vec<u8>>, SecureVaultError> {
    let mut key = Zeroizing::new(vec![0u8; KEY_LENGTH_BYTES]);
    let output = NonNull::from(&mut key[0]).cast::<c_void>();

    // SAFETY: kSecRandomDefault is the supported immutable generator and the
    // output points to a live writable 32-byte Zeroizing allocation.
    let random_status = unsafe { SecRandomCopyBytes(kSecRandomDefault, key.len(), output) };
    if random_status != errSecSuccess {
        return Err(SecureVaultError::RandomGenerationFailed {
            status: random_status,
        });
    }

    let access_control = create_access_control()?;
    let add_status = {
        // Avoid a second plaintext allocation. Core Foundation borrows the
        // Zeroizing buffer only for this synchronous SecItemAdd scope and must
        // not deallocate it (`kCFAllocatorNull`).
        // SAFETY: `key` remains alive and immutable until both the attributes
        // dictionary and CFData view are dropped at the end of this block.
        let key_data = unsafe {
            CFData::with_bytes_no_copy(
                None,
                key.as_ptr(),
                KEY_LENGTH_BYTES as CFIndex,
                kCFAllocatorNull,
            )
        }
        .ok_or(SecureVaultError::KeyDataViewCreationFailed)?;

        let attributes = build_add_attributes(&key_data, &access_control);
        // SAFETY: The attributes contain only documented typed values. No
        // result is requested, so a null output pointer is valid.
        unsafe { SecItemAdd(as_cf_dictionary(&attributes), ptr::null_mut()) }
    };

    if add_status == errSecSuccess {
        Ok(key)
    } else if add_status == errSecDuplicateItem {
        // Another caller won the first-use race. Destroy our generated key
        // before retrieving the canonical Keychain value.
        drop(key);
        match retrieve_key(context)? {
            LookupOutcome::Found(existing) => Ok(existing),
            LookupOutcome::NotFound => Err(SecureVaultError::DuplicateItemRace),
        }
    } else {
        Err(map_keychain_status(KeychainOperation::Add, add_status))
    }
}

fn create_access_control() -> Result<CFRetained<SecAccessControl>, SecureVaultError> {
    // SAFETY: The exported accessibility constant has the required CFType,
    // the error output is allowed to be null, and the Create-rule result is
    // returned as a typed CFRetained owner by the generated binding.
    unsafe {
        SecAccessControl::with_flags(
            None,
            kSecAttrAccessibleWhenPasscodeSetThisDeviceOnly.as_ref(),
            SecAccessControlCreateFlags::UserPresence,
            ptr::null_mut(),
        )
    }
    .ok_or(SecureVaultError::AccessControlCreationFailed)
}

fn build_retrieval_query(
    context: &Retained<LAContext>,
) -> Retained<NSMutableDictionary<NSString, AnyObject>> {
    let query = NSMutableDictionary::<NSString, AnyObject>::new();
    let service = NSString::from_str(KEYCHAIN_SERVICE);
    let account = NSString::from_str(KEYCHAIN_ACCOUNT);
    let synchronizable = NSNumber::new_bool(false);
    let return_data = NSNumber::new_bool(true);

    // SAFETY: Framework-exported CFStrings are immutable typed constants.
    // Foundation copies each key and retains each typed value, including the
    // exact LAContext supplied by the caller.
    unsafe {
        let class_key: &NSString = kSecClass.as_ref();
        let class_value: &AnyObject = kSecClassGenericPassword.as_ref();
        query.insert(class_key, class_value);

        let service_key: &NSString = kSecAttrService.as_ref();
        let service_value: &AnyObject = service.as_ref();
        query.insert(service_key, service_value);

        let account_key: &NSString = kSecAttrAccount.as_ref();
        let account_value: &AnyObject = account.as_ref();
        query.insert(account_key, account_value);

        let synchronizable_key: &NSString = kSecAttrSynchronizable.as_ref();
        let synchronizable_value: &AnyObject = synchronizable.as_ref();
        query.insert(synchronizable_key, synchronizable_value);

        let return_data_key: &NSString = kSecReturnData.as_ref();
        let return_data_value: &AnyObject = return_data.as_ref();
        query.insert(return_data_key, return_data_value);

        let match_limit_key: &NSString = kSecMatchLimit.as_ref();
        let match_limit_value: &AnyObject = kSecMatchLimitOne.as_ref();
        query.insert(match_limit_key, match_limit_value);

        let auth_context_key: &NSString = kSecUseAuthenticationContext.as_ref();
        let auth_context_value: &AnyObject = context.as_ref();
        query.insert(auth_context_key, auth_context_value);
    }

    query
}

fn build_add_attributes(
    key_data: &CFData,
    access_control: &SecAccessControl,
) -> Retained<NSMutableDictionary<NSString, AnyObject>> {
    let attributes = NSMutableDictionary::<NSString, AnyObject>::new();
    let service = NSString::from_str(KEYCHAIN_SERVICE);
    let account = NSString::from_str(KEYCHAIN_ACCOUNT);
    let synchronizable = NSNumber::new_bool(false);

    // SAFETY: Framework-exported CFStrings are immutable typed constants.
    // The dictionary copies keys and retains each value for its own lifetime.
    // CFData and SecAccessControl are typed objc2/CoreFoundation objects.
    unsafe {
        let class_key: &NSString = kSecClass.as_ref();
        let class_value: &AnyObject = kSecClassGenericPassword.as_ref();
        attributes.insert(class_key, class_value);

        let service_key: &NSString = kSecAttrService.as_ref();
        let service_value: &AnyObject = service.as_ref();
        attributes.insert(service_key, service_value);

        let account_key: &NSString = kSecAttrAccount.as_ref();
        let account_value: &AnyObject = account.as_ref();
        attributes.insert(account_key, account_value);

        let synchronizable_key: &NSString = kSecAttrSynchronizable.as_ref();
        let synchronizable_value: &AnyObject = synchronizable.as_ref();
        attributes.insert(synchronizable_key, synchronizable_value);

        let value_data_key: &NSString = kSecValueData.as_ref();
        let value_data: &AnyObject = key_data.as_ref();
        attributes.insert(value_data_key, value_data);

        let access_control_key: &NSString = kSecAttrAccessControl.as_ref();
        let access_control_value: &AnyObject = access_control.as_ref();
        attributes.insert(access_control_key, access_control_value);
    }

    attributes
}

fn as_cf_dictionary(dictionary: &NSMutableDictionary<NSString, AnyObject>) -> &CFDictionary {
    let mutable: &CFMutableDictionary<NSString, AnyObject> = dictionary.as_ref();
    mutable.as_opaque()
}

fn validate_key_length(key: &[u8]) -> Result<(), SecureVaultError> {
    if key.len() == KEY_LENGTH_BYTES {
        Ok(())
    } else {
        Err(SecureVaultError::InvalidKeyLength { actual: key.len() })
    }
}

fn map_keychain_status(operation: KeychainOperation, status: i32) -> SecureVaultError {
    if status == errSecUserCanceled {
        SecureVaultError::AuthenticationCancelled
    } else if status == errSecAuthFailed {
        SecureVaultError::AuthenticationFailed
    } else if status == errSecInteractionNotAllowed {
        SecureVaultError::InteractionNotAllowed
    } else {
        SecureVaultError::KeychainStatus { operation, status }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_exact_key_length() {
        assert_eq!(validate_key_length(&[0u8; KEY_LENGTH_BYTES]), Ok(()));
        assert_eq!(
            validate_key_length(&[0u8; KEY_LENGTH_BYTES - 1]),
            Err(SecureVaultError::InvalidKeyLength {
                actual: KEY_LENGTH_BYTES - 1
            })
        );
    }

    #[test]
    fn maps_authentication_statuses_without_exposing_key_material() {
        assert_eq!(
            map_keychain_status(KeychainOperation::Retrieve, errSecUserCanceled),
            SecureVaultError::AuthenticationCancelled
        );
        assert_eq!(
            map_keychain_status(KeychainOperation::Retrieve, errSecAuthFailed),
            SecureVaultError::AuthenticationFailed
        );
        assert_eq!(
            map_keychain_status(KeychainOperation::Retrieve, errSecInteractionNotAllowed),
            SecureVaultError::InteractionNotAllowed
        );
    }

    #[test]
    fn preserves_unknown_status_with_operation_context() {
        assert_eq!(
            map_keychain_status(KeychainOperation::Add, -50),
            SecureVaultError::KeychainStatus {
                operation: KeychainOperation::Add,
                status: -50
            }
        );
    }
}
