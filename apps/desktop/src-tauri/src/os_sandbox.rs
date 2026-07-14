use std::fmt;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, Command};

#[derive(Debug)]
pub struct SandboxUnavailable(String);

impl SandboxUnavailable {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for SandboxUnavailable {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

pub trait ChildControl: Send {
    fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>>;
    fn wait(&mut self) -> std::io::Result<std::process::ExitStatus>;
    fn kill(&mut self) -> std::io::Result<()>;
}

impl ChildControl for Child {
    fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        Child::try_wait(self)
    }

    fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        Child::wait(self)
    }

    fn kill(&mut self) -> std::io::Result<()> {
        Child::kill(self)
    }
}

pub struct SandboxedProcess {
    pub child: Box<dyn ChildControl>,
    pub stdin: Box<dyn Write + Send>,
    pub stdout: Box<dyn Read + Send>,
    pub stderr: Box<dyn Read + Send>,
}

pub fn spawn_kernel_sandboxed(
    command: Command,
    data_root: &Path,
) -> Result<SandboxedProcess, SandboxUnavailable> {
    platform::spawn(command, data_root)
}

#[cfg(not(windows))]
fn take_standard_child(mut child: Child) -> Result<SandboxedProcess, SandboxUnavailable> {
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| SandboxUnavailable::new("sandboxed child stdin was not created"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| SandboxUnavailable::new("sandboxed child stdout was not created"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| SandboxUnavailable::new("sandboxed child stderr was not created"))?;
    Ok(SandboxedProcess {
        child: Box::new(child),
        stdin: Box::new(stdin),
        stdout: Box::new(stdout),
        stderr: Box::new(stderr),
    })
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use std::os::unix::process::CommandExt;

    const AUDIT_ARCH_X86_64: u32 = 0xC000_003E;
    const AUDIT_ARCH_AARCH64: u32 = 0xC000_00B7;

    pub(super) fn spawn(
        mut command: Command,
        _data_root: &Path,
    ) -> Result<SandboxedProcess, SandboxUnavailable> {
        let audit_arch = if cfg!(target_arch = "x86_64") {
            AUDIT_ARCH_X86_64
        } else if cfg!(target_arch = "aarch64") {
            AUDIT_ARCH_AARCH64
        } else {
            return Err(SandboxUnavailable::new(
                "unsupported Linux audit architecture",
            ));
        };

        unsafe {
            command.pre_exec(move || install_seccomp(audit_arch));
        }
        let child = command.spawn().map_err(|error| {
            SandboxUnavailable::new(format!("seccomp child launch failed: {error}"))
        })?;
        take_standard_child(child)
    }

    unsafe fn install_seccomp(audit_arch: u32) -> std::io::Result<()> {
        const SECCOMP_MODE_FILTER: libc::c_ulong = 2;
        const SECCOMP_RET_KILL_PROCESS: u32 = 0x8000_0000;
        const SECCOMP_RET_ERRNO: u32 = 0x0005_0000;
        const SECCOMP_RET_ALLOW: u32 = 0x7fff_0000;
        const BPF_LD: u16 = 0x00;
        const BPF_W: u16 = 0x00;
        const BPF_ABS: u16 = 0x20;
        const BPF_JMP: u16 = 0x05;
        const BPF_JEQ: u16 = 0x10;
        const BPF_K: u16 = 0x00;
        const BPF_RET: u16 = 0x06;

        fn stmt(code: u16, k: u32) -> libc::sock_filter {
            libc::sock_filter {
                code,
                jt: 0,
                jf: 0,
                k,
            }
        }
        fn jump(code: u16, k: u32, jt: u8, jf: u8) -> libc::sock_filter {
            libc::sock_filter { code, jt, jf, k }
        }

        // struct seccomp_data: nr @ 0, arch @ 4, args[0] @ 16.
        let mut filter = [
            stmt(BPF_LD | BPF_W | BPF_ABS, 4),
            jump(BPF_JMP | BPF_JEQ | BPF_K, audit_arch, 1, 0),
            stmt(BPF_RET | BPF_K, SECCOMP_RET_KILL_PROCESS),
            stmt(BPF_LD | BPF_W | BPF_ABS, 0),
            jump(BPF_JMP | BPF_JEQ | BPF_K, libc::SYS_socket as u32, 0, 3),
            stmt(BPF_LD | BPF_W | BPF_ABS, 16),
            jump(BPF_JMP | BPF_JEQ | BPF_K, libc::AF_INET as u32, 2, 0),
            jump(BPF_JMP | BPF_JEQ | BPF_K, libc::AF_INET6 as u32, 1, 0),
            stmt(BPF_RET | BPF_K, SECCOMP_RET_ALLOW),
            stmt(BPF_RET | BPF_K, SECCOMP_RET_ERRNO | (libc::EACCES as u32)),
        ];
        let program = libc::sock_fprog {
            len: filter.len() as u16,
            filter: filter.as_mut_ptr(),
        };
        if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
            return Err(std::io::Error::last_os_error());
        }
        if libc::prctl(
            libc::PR_SET_SECCOMP,
            SECCOMP_MODE_FILTER,
            &program as *const libc::sock_fprog,
        ) != 0
        {
            return Err(std::io::Error::last_os_error());
        }
        let _ = libc::AF_UNIX;
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use std::ffi::{c_char, c_void, CString};

    type CfObject = *const c_void;
    const UTF8_ENCODING: u32 = 0x0800_0100;
    const APP_CONTAINER_ID: &str = "com.ai-shizu.pkb";

    #[link(name = "Security", kind = "framework")]
    unsafe extern "C" {
        fn SecTaskCreateFromSelf(allocator: CfObject) -> CfObject;
        fn SecTaskCopyValueForEntitlement(
            task: CfObject,
            entitlement: CfObject,
            error: *mut CfObject,
        ) -> CfObject;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFStringCreateWithCString(
            allocator: CfObject,
            text: *const c_char,
            encoding: u32,
        ) -> CfObject;
        fn CFGetTypeID(value: CfObject) -> usize;
        fn CFBooleanGetTypeID() -> usize;
        fn CFBooleanGetValue(value: CfObject) -> u8;
        fn CFRelease(value: CfObject);
    }

    pub(super) fn spawn(
        mut command: Command,
        _data_root: &Path,
    ) -> Result<SandboxedProcess, SandboxUnavailable> {
        if std::env::var("APP_SANDBOX_CONTAINER_ID").as_deref() != Ok(APP_CONTAINER_ID)
            || !macos_app_sandbox_entitlement_is_active()
        {
            return Err(SandboxUnavailable::new(
                "macOS App Sandbox entitlement is not active",
            ));
        }
        let child = command.spawn().map_err(|error| {
            SandboxUnavailable::new(format!("sandboxed child launch failed: {error}"))
        })?;
        take_standard_child(child)
    }

    fn macos_app_sandbox_entitlement_is_active() -> bool {
        unsafe {
            let task = SecTaskCreateFromSelf(std::ptr::null());
            if task.is_null() {
                return false;
            }
            let name =
                CString::new("com.apple.security.app-sandbox").expect("fixed entitlement name");
            let key = CFStringCreateWithCString(std::ptr::null(), name.as_ptr(), UTF8_ENCODING);
            if key.is_null() {
                CFRelease(task);
                return false;
            }
            let value = SecTaskCopyValueForEntitlement(task, key, std::ptr::null_mut());
            let active = !value.is_null()
                && CFGetTypeID(value) == CFBooleanGetTypeID()
                && CFBooleanGetValue(value) != 0;
            if !value.is_null() {
                CFRelease(value);
            }
            CFRelease(key);
            CFRelease(task);
            active
        }
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::collections::BTreeMap;
    use std::ffi::{c_void, OsStr, OsString};
    use std::fs::File;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::os::windows::fs::MetadataExt;
    use std::os::windows::io::{FromRawHandle, RawHandle};
    use std::os::windows::process::ExitStatusExt;
    use std::ptr::{null, null_mut};

    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, LocalFree, SetHandleInformation, GENERIC_ALL, GENERIC_EXECUTE,
        GENERIC_READ, HANDLE, HANDLE_FLAG_INHERIT, STILL_ACTIVE,
    };
    use windows_sys::Win32::Security::Authorization::{
        BuildTrusteeWithSidW, ConvertSidToStringSidW, GetNamedSecurityInfoW, SetEntriesInAclW,
        SetNamedSecurityInfoW, EXPLICIT_ACCESS_W, GRANT_ACCESS, SE_FILE_OBJECT,
    };
    use windows_sys::Win32::Security::Isolation::{
        CreateAppContainerProfile, DeriveAppContainerSidFromAppContainerName,
        GetAppContainerFolderPath,
    };
    use windows_sys::Win32::Security::{
        FreeSid, ACL, DACL_SECURITY_INFORMATION, NO_INHERITANCE, PSECURITY_DESCRIPTOR, PSID,
        SECURITY_ATTRIBUTES, SECURITY_CAPABILITIES, SUB_CONTAINERS_AND_OBJECTS_INHERIT,
    };
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::System::Pipes::CreatePipe;
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
        InitializeProcThreadAttributeList, TerminateProcess, UpdateProcThreadAttribute,
        WaitForSingleObject, CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT,
        EXTENDED_STARTUPINFO_PRESENT, INFINITE, PROCESS_INFORMATION,
        PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES,
        STARTF_USESTDHANDLES, STARTUPINFOEXW,
    };

    const PROFILE_NAME: &str = "PKB.Engine.NoNetwork";
    const ERROR_ALREADY_EXISTS_HRESULT: i32 = 0x8007_00B7u32 as i32;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;

    struct OwnedHandle(HANDLE);

    // A process HANDLE is an owned kernel reference and may be waited or closed
    // from another thread. Ownership remains unique through this wrapper.
    unsafe impl Send for OwnedHandle {}

    impl OwnedHandle {
        fn null() -> Self {
            Self(null_mut())
        }

        fn into_file(mut self) -> File {
            let handle = self.0;
            self.0 = null_mut();
            unsafe { File::from_raw_handle(handle as RawHandle) }
        }
    }

    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    CloseHandle(self.0);
                }
            }
        }
    }

    struct OwnedSid(PSID);

    impl Drop for OwnedSid {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    FreeSid(self.0);
                }
            }
        }
    }

    struct AttributeList {
        storage: Vec<usize>,
    }

    impl AttributeList {
        fn as_mut_ptr(&mut self) -> *mut c_void {
            self.storage.as_mut_ptr().cast()
        }
    }

    impl Drop for AttributeList {
        fn drop(&mut self) {
            if !self.storage.is_empty() {
                unsafe {
                    DeleteProcThreadAttributeList(self.as_mut_ptr());
                }
            }
        }
    }

    struct AppContainerChild {
        process: OwnedHandle,
        exit_code: Option<u32>,
    }

    impl ChildControl for AppContainerChild {
        fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
            if let Some(code) = self.exit_code {
                return Ok(Some(std::process::ExitStatus::from_raw(code)));
            }
            let mut code = 0u32;
            if unsafe { GetExitCodeProcess(self.process.0, &mut code) } == 0 {
                return Err(std::io::Error::last_os_error());
            }
            if code == STILL_ACTIVE as u32 {
                Ok(None)
            } else {
                self.exit_code = Some(code);
                Ok(Some(std::process::ExitStatus::from_raw(code)))
            }
        }

        fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
            if let Some(status) = self.try_wait()? {
                return Ok(status);
            }
            let result = unsafe { WaitForSingleObject(self.process.0, INFINITE) };
            if result != 0 {
                return Err(std::io::Error::last_os_error());
            }
            self.try_wait()?.ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "AppContainer process remained active after wait",
                )
            })
        }

        fn kill(&mut self) -> std::io::Result<()> {
            if self.try_wait()?.is_some() {
                return Ok(());
            }
            if unsafe { TerminateProcess(self.process.0, 1) } == 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        }
    }

    pub(super) fn spawn(
        command: Command,
        data_root: &Path,
    ) -> Result<SandboxedProcess, SandboxUnavailable> {
        unsafe { spawn_appcontainer(command, data_root) }
    }

    unsafe fn spawn_appcontainer(
        command: Command,
        data_root: &Path,
    ) -> Result<SandboxedProcess, SandboxUnavailable> {
        let profile_name = wide_nul(OsStr::new(PROFILE_NAME));
        let display_name = wide_nul(OsStr::new("PKB Engine Network Isolation"));
        let description = wide_nul(OsStr::new(
            "PKB private engine AppContainer with zero network capabilities",
        ));
        let mut raw_sid: PSID = null_mut();
        let created = CreateAppContainerProfile(
            profile_name.as_ptr(),
            display_name.as_ptr(),
            description.as_ptr(),
            null(),
            0,
            &mut raw_sid,
        );
        if created == ERROR_ALREADY_EXISTS_HRESULT {
            let derived =
                DeriveAppContainerSidFromAppContainerName(profile_name.as_ptr(), &mut raw_sid);
            check_hresult(derived, "derive AppContainer SID")?;
        } else {
            check_hresult(created, "create AppContainer profile")?;
        }
        if raw_sid.is_null() {
            return Err(SandboxUnavailable::new("AppContainer SID was null"));
        }
        let sid = OwnedSid(raw_sid);
        let sid_text = sid_to_string(sid.0)?;
        let container_root = appcontainer_folder(&sid_text)?;
        let container_temp = container_root.join("Temp");
        std::fs::create_dir_all(&container_temp).map_err(|error| {
            SandboxUnavailable::new(format!("create AppContainer temp directory: {error}"))
        })?;

        std::fs::create_dir_all(data_root)
            .map_err(|error| SandboxUnavailable::new(format!("create PKB data root: {error}")))?;
        let canonical_data_root = canonical_non_reparse_path(data_root, "PKB data root")?;
        let canonical_program = canonical_non_reparse_path(
            Path::new(command.get_program()),
            "bundled engine executable",
        )?;
        grant_path_access(&canonical_data_root, sid.0, GENERIC_ALL, true)?;
        grant_path_access(
            &canonical_program,
            sid.0,
            GENERIC_READ | GENERIC_EXECUTE,
            false,
        )?;

        let mut child_stdin_read = OwnedHandle::null();
        let mut parent_stdin_write = OwnedHandle::null();
        let mut parent_stdout_read = OwnedHandle::null();
        let mut child_stdout_write = OwnedHandle::null();
        let mut parent_stderr_read = OwnedHandle::null();
        let mut child_stderr_write = OwnedHandle::null();
        create_parent_child_pipe(&mut child_stdin_read, &mut parent_stdin_write)?;
        create_parent_child_pipe(&mut parent_stdout_read, &mut child_stdout_write)?;
        create_parent_child_pipe(&mut parent_stderr_read, &mut child_stderr_write)?;
        make_non_inheritable(parent_stdin_write.0)?;
        make_non_inheritable(parent_stdout_read.0)?;
        make_non_inheritable(parent_stderr_read.0)?;

        let mut attribute_size = 0usize;
        InitializeProcThreadAttributeList(null_mut(), 2, 0, &mut attribute_size);
        if attribute_size == 0 {
            return Err(last_error("size process attribute list"));
        }
        let words =
            (attribute_size + std::mem::size_of::<usize>() - 1) / std::mem::size_of::<usize>();
        let mut attributes = AttributeList {
            storage: vec![0usize; words],
        };
        if InitializeProcThreadAttributeList(attributes.as_mut_ptr(), 2, 0, &mut attribute_size)
            == 0
        {
            return Err(last_error("initialize process attribute list"));
        }

        let capabilities = SECURITY_CAPABILITIES {
            AppContainerSid: sid.0,
            Capabilities: null_mut(),
            CapabilityCount: 0,
            Reserved: 0,
        };
        if UpdateProcThreadAttribute(
            attributes.as_mut_ptr(),
            0,
            PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
            (&capabilities as *const SECURITY_CAPABILITIES).cast(),
            std::mem::size_of::<SECURITY_CAPABILITIES>(),
            null_mut(),
            null(),
        ) == 0
        {
            return Err(last_error("apply zero-capability AppContainer"));
        }
        let inherited_handles = [
            child_stdin_read.0,
            child_stdout_write.0,
            child_stderr_write.0,
        ];
        if UpdateProcThreadAttribute(
            attributes.as_mut_ptr(),
            0,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            inherited_handles.as_ptr().cast(),
            std::mem::size_of_val(&inherited_handles),
            null_mut(),
            null(),
        ) == 0
        {
            return Err(last_error("apply inherited handle allowlist"));
        }

        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = child_stdin_read.0;
        startup.StartupInfo.hStdOutput = child_stdout_write.0;
        startup.StartupInfo.hStdError = child_stderr_write.0;
        startup.lpAttributeList = attributes.as_mut_ptr();

        let program = wide_nul(canonical_program.as_os_str());
        let mut command_line = command_line(&command);
        let cwd = wide_nul(
            command
                .get_current_dir()
                .unwrap_or(&canonical_data_root)
                .as_os_str(),
        );
        let mut environment =
            command_environment(&command, &container_root, &container_temp, &sid_text)?;
        let mut process_info = PROCESS_INFORMATION::default();
        if CreateProcessW(
            program.as_ptr(),
            command_line.as_mut_ptr(),
            null(),
            null(),
            1,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT | CREATE_NO_WINDOW,
            environment.as_mut_ptr().cast(),
            cwd.as_ptr(),
            &startup.StartupInfo as *const _,
            &mut process_info,
        ) == 0
        {
            return Err(last_error("launch AppContainer engine"));
        }
        let process = OwnedHandle(process_info.hProcess);
        CloseHandle(process_info.hThread);
        drop(child_stdin_read);
        drop(child_stdout_write);
        drop(child_stderr_write);

        Ok(SandboxedProcess {
            child: Box::new(AppContainerChild {
                process,
                exit_code: None,
            }),
            stdin: Box::new(parent_stdin_write.into_file()),
            stdout: Box::new(parent_stdout_read.into_file()),
            stderr: Box::new(parent_stderr_read.into_file()),
        })
    }

    unsafe fn create_parent_child_pipe(
        read: &mut OwnedHandle,
        write: &mut OwnedHandle,
    ) -> Result<(), SandboxUnavailable> {
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: null_mut(),
            bInheritHandle: 1,
        };
        if CreatePipe(&mut read.0, &mut write.0, &attributes, 0) == 0 {
            return Err(last_error("create child stdio pipe"));
        }
        Ok(())
    }

    unsafe fn make_non_inheritable(handle: HANDLE) -> Result<(), SandboxUnavailable> {
        if SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) == 0 {
            return Err(last_error("protect parent stdio handle"));
        }
        Ok(())
    }

    unsafe fn sid_to_string(sid: PSID) -> Result<OsString, SandboxUnavailable> {
        let mut raw = null_mut();
        if ConvertSidToStringSidW(sid, &mut raw) == 0 {
            return Err(last_error("convert AppContainer SID"));
        }
        let result = pwstr_to_os_string(raw);
        LocalFree(raw.cast());
        Ok(result)
    }

    unsafe fn appcontainer_folder(sid: &OsStr) -> Result<std::path::PathBuf, SandboxUnavailable> {
        let sid_wide = wide_nul(sid);
        let mut raw = null_mut();
        let result = GetAppContainerFolderPath(sid_wide.as_ptr(), &mut raw);
        check_hresult(result, "resolve AppContainer folder")?;
        if raw.is_null() {
            return Err(SandboxUnavailable::new("AppContainer folder was null"));
        }
        let path = std::path::PathBuf::from(pwstr_to_os_string(raw));
        CoTaskMemFree(raw.cast());
        Ok(path)
    }

    unsafe fn grant_path_access(
        path: &Path,
        sid: PSID,
        access: u32,
        inherit: bool,
    ) -> Result<(), SandboxUnavailable> {
        let mut path_wide = wide_nul(path.as_os_str());
        let mut old_dacl: *mut ACL = null_mut();
        let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
        let status = GetNamedSecurityInfoW(
            path_wide.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            &mut old_dacl,
            null_mut(),
            &mut descriptor,
        );
        if status != 0 {
            return Err(SandboxUnavailable::new(format!(
                "read ACL for {}: Win32 {status}",
                path.display()
            )));
        }
        let mut entry = EXPLICIT_ACCESS_W {
            grfAccessPermissions: access,
            grfAccessMode: GRANT_ACCESS,
            grfInheritance: if inherit {
                SUB_CONTAINERS_AND_OBJECTS_INHERIT
            } else {
                NO_INHERITANCE
            },
            ..Default::default()
        };
        BuildTrusteeWithSidW(&mut entry.Trustee, sid);
        let mut new_dacl: *mut ACL = null_mut();
        let merge_status = SetEntriesInAclW(1, &entry, old_dacl, &mut new_dacl);
        if merge_status != 0 {
            LocalFree(descriptor.cast());
            return Err(SandboxUnavailable::new(format!(
                "build ACL for {}: Win32 {merge_status}",
                path.display()
            )));
        }
        let set_status = SetNamedSecurityInfoW(
            path_wide.as_mut_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            new_dacl,
            null_mut(),
        );
        LocalFree(new_dacl.cast());
        LocalFree(descriptor.cast());
        if set_status != 0 {
            return Err(SandboxUnavailable::new(format!(
                "set ACL for {}: Win32 {set_status}",
                path.display()
            )));
        }
        Ok(())
    }

    fn canonical_non_reparse_path(
        path: &Path,
        label: &str,
    ) -> Result<std::path::PathBuf, SandboxUnavailable> {
        let metadata = std::fs::symlink_metadata(path)
            .map_err(|error| SandboxUnavailable::new(format!("inspect {label}: {error}")))?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(SandboxUnavailable::new(format!(
                "{label} must not be a reparse point"
            )));
        }
        path.canonicalize()
            .map_err(|error| SandboxUnavailable::new(format!("canonicalize {label}: {error}")))
    }

    fn command_environment(
        command: &Command,
        container_root: &Path,
        container_temp: &Path,
        sid: &OsStr,
    ) -> Result<Vec<u16>, SandboxUnavailable> {
        let mut values: BTreeMap<Vec<u16>, (OsString, OsString)> = BTreeMap::new();
        for (key, value) in command.get_envs() {
            let value = value.ok_or_else(|| {
                SandboxUnavailable::new("removed variables are not valid in an isolated block")
            })?;
            insert_environment(&mut values, key, value);
        }
        for (key, value) in [
            (OsStr::new("LOCALAPPDATA"), container_root.as_os_str()),
            (OsStr::new("APPDATA"), container_root.as_os_str()),
            (OsStr::new("TEMP"), container_temp.as_os_str()),
            (OsStr::new("TMP"), container_temp.as_os_str()),
            (OsStr::new("PKB_APPCONTAINER_SID"), sid),
        ] {
            insert_environment(&mut values, key, value);
        }
        let mut block = Vec::new();
        for (_, (key, value)) in values {
            block.extend(key.encode_wide());
            block.push(b'=' as u16);
            block.extend(value.encode_wide());
            block.push(0);
        }
        block.push(0);
        Ok(block)
    }

    fn insert_environment(
        values: &mut BTreeMap<Vec<u16>, (OsString, OsString)>,
        key: &OsStr,
        value: &OsStr,
    ) {
        let sort_key = key
            .encode_wide()
            .map(|unit| {
                if (b'A' as u16..=b'Z' as u16).contains(&unit) {
                    unit + 32
                } else {
                    unit
                }
            })
            .collect();
        values.insert(sort_key, (key.to_os_string(), value.to_os_string()));
    }

    fn command_line(command: &Command) -> Vec<u16> {
        let mut result = quote_argument(command.get_program());
        for argument in command.get_args() {
            result.push(b' ' as u16);
            result.extend(quote_argument(argument));
        }
        result.push(0);
        result
    }

    fn quote_argument(argument: &OsStr) -> Vec<u16> {
        let units: Vec<u16> = argument.encode_wide().collect();
        let needs_quotes =
            units.is_empty() || units.iter().any(|unit| matches!(*unit, 0x20 | 0x09 | 0x22));
        if !needs_quotes {
            return units;
        }
        let mut result = vec![b'"' as u16];
        let mut backslashes = 0usize;
        for unit in units {
            if unit == b'\\' as u16 {
                backslashes += 1;
            } else if unit == b'"' as u16 {
                result.extend(std::iter::repeat_n(b'\\' as u16, backslashes * 2 + 1));
                result.push(unit);
                backslashes = 0;
            } else {
                result.extend(std::iter::repeat_n(b'\\' as u16, backslashes));
                backslashes = 0;
                result.push(unit);
            }
        }
        result.extend(std::iter::repeat_n(b'\\' as u16, backslashes * 2));
        result.push(b'"' as u16);
        result
    }

    fn wide_nul(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(std::iter::once(0)).collect()
    }

    unsafe fn pwstr_to_os_string(pointer: *const u16) -> OsString {
        let mut length = 0usize;
        while *pointer.add(length) != 0 {
            length += 1;
        }
        OsString::from_wide(std::slice::from_raw_parts(pointer, length))
    }

    fn check_hresult(result: i32, operation: &str) -> Result<(), SandboxUnavailable> {
        if result < 0 {
            Err(SandboxUnavailable::new(format!(
                "{operation}: HRESULT 0x{:08X}",
                result as u32
            )))
        } else {
            Ok(())
        }
    }

    fn last_error(operation: &str) -> SandboxUnavailable {
        let code = unsafe { GetLastError() };
        SandboxUnavailable::new(format!("{operation}: Win32 {code}"))
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
mod platform {
    use super::*;

    pub(super) fn spawn(
        _command: Command,
        _data_root: &Path,
    ) -> Result<SandboxedProcess, SandboxUnavailable> {
        Err(SandboxUnavailable::new("unsupported operating system"))
    }
}
