//! Raw jetsam footprint probe (docs/architecture_blueprint.md §3.2).
//!
//! Returns `phys_footprint` — the exact value the iOS jetsam ledger accounts
//! against — via `proc_pid_rusage(RUSAGE_INFO_V2).ri_phys_footprint`. The M0 spike
//! rejected `task_info(MACH_TASK_BASIC_INFO)` because that flavor only exposes
//! `resident_size`, not phys_footprint.

/// Current process physical footprint in bytes (jetsam-accounted), or `None` when
/// unavailable.
#[cfg(target_vendor = "apple")]
pub fn phys_footprint_bytes() -> Option<u64> {
    // SAFETY: `proc_pid_rusage` fills a `rusage_info_v2` when called with
    // `RUSAGE_INFO_V2`. We pass a zeroed, correctly-typed buffer and only read it
    // back when the call reports success (rc == 0).
    unsafe {
        let mut info: libc::rusage_info_v2 = std::mem::zeroed();
        let rc = libc::proc_pid_rusage(
            libc::getpid(),
            libc::RUSAGE_INFO_V2,
            (&mut info as *mut libc::rusage_info_v2).cast::<libc::rusage_info_t>(),
        );
        if rc == 0 {
            Some(info.ri_phys_footprint)
        } else {
            None
        }
    }
}

#[cfg(not(target_vendor = "apple"))]
pub fn phys_footprint_bytes() -> Option<u64> {
    None
}
