fn main() {
    // Fail-closed: exit 1 unless this target proves IP sockets are denied.
    // Unsupported targets compile but must never look like a successful probe.
    if !native_ip_sockets_are_denied() {
        std::process::exit(1);
    }
}

#[cfg(windows)]
fn native_ip_sockets_are_denied() -> bool {
    if !windows_token_is_zero_capability_appcontainer() {
        return false;
    }
    use windows_sys::Win32::Networking::WinSock::{
        closesocket, connect, getsockopt, ioctlsocket, socket, WSACleanup, WSAGetLastError,
        WSAPoll, WSAStartup, AF_INET, AF_INET6, FIONBIO, INVALID_SOCKET, IPPROTO_TCP, POLLOUT,
        SOCKADDR, SOCKET_ERROR, SOCK_STREAM, SOL_SOCKET, SO_ERROR, WSADATA, WSAEACCES,
        WSAEAFNOSUPPORT, WSAEWOULDBLOCK, WSAPOLLFD,
    };

    #[repr(C)]
    struct SocketAddressV4 {
        family: u16,
        port: u16,
        address: [u8; 4],
        zero: [u8; 8],
    }

    #[repr(C)]
    struct SocketAddressV6 {
        family: u16,
        port: u16,
        flow_info: u32,
        address: [u8; 16],
        scope_id: u32,
    }

    let mut data = WSADATA::default();
    if unsafe { WSAStartup(0x0202, &mut data) } != 0 {
        return false;
    }
    let mut denied = true;
    for family in [AF_INET, AF_INET6] {
        let handle = unsafe { socket(family.into(), SOCK_STREAM, IPPROTO_TCP) };
        if handle == INVALID_SOCKET {
            let error = unsafe { WSAGetLastError() };
            eprintln!("family={family} socket_error={error}");
            denied &= matches!(error, WSAEACCES | WSAEAFNOSUPPORT);
            continue;
        }
        let mut nonblocking = 1u32;
        if unsafe { ioctlsocket(handle, FIONBIO, &mut nonblocking) } != 0 {
            unsafe {
                closesocket(handle);
            }
            denied = false;
            continue;
        }
        let result = if family == AF_INET {
            let address = SocketAddressV4 {
                family: AF_INET,
                port: probe_port("PKB_KERNEL_SANDBOX_PROBE_PORT_V4").to_be(),
                address: [127, 0, 0, 1],
                zero: [0; 8],
            };
            unsafe {
                connect(
                    handle,
                    (&address as *const SocketAddressV4).cast::<SOCKADDR>(),
                    std::mem::size_of::<SocketAddressV4>() as i32,
                )
            }
        } else {
            let mut loopback = [0u8; 16];
            loopback[15] = 1;
            let address = SocketAddressV6 {
                family: AF_INET6,
                port: probe_port("PKB_KERNEL_SANDBOX_PROBE_PORT_V6").to_be(),
                flow_info: 0,
                address: loopback,
                scope_id: 0,
            };
            unsafe {
                connect(
                    handle,
                    (&address as *const SocketAddressV6).cast::<SOCKADDR>(),
                    std::mem::size_of::<SocketAddressV6>() as i32,
                )
            }
        };
        let initial_error = unsafe { WSAGetLastError() };
        let mut poll = WSAPOLLFD {
            fd: handle,
            events: POLLOUT,
            revents: 0,
        };
        let poll_result = unsafe { WSAPoll(&mut poll, 1, 2_000) };
        let mut completion_error = 0i32;
        let mut completion_len = std::mem::size_of::<i32>() as i32;
        let option_result = unsafe {
            getsockopt(
                handle,
                SOL_SOCKET,
                SO_ERROR,
                (&mut completion_error as *mut i32).cast(),
                &mut completion_len,
            )
        };
        eprintln!(
            "family={family} connect_result={result} initial_error={initial_error} poll={poll_result} completion={completion_error}"
        );
        unsafe {
            closesocket(handle);
        }
        let denied_immediately =
            poll_result > 0 && option_result == 0 && completion_error == WSAEACCES;
        let denied_by_isolation_timeout =
            poll_result == 0 && initial_error == WSAEWOULDBLOCK && completion_error == 0;
        denied &= result == SOCKET_ERROR && (denied_immediately || denied_by_isolation_timeout);
    }
    unsafe {
        WSACleanup();
    }
    denied
}

#[cfg(windows)]
fn windows_token_is_zero_capability_appcontainer() -> bool {
    use std::ffi::c_void;
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{
        GetTokenInformation, TokenCapabilities, TokenIsAppContainer, TOKEN_GROUPS, TOKEN_QUERY,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    let mut token: HANDLE = null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return false;
    }
    let mut is_container = 0u32;
    let mut returned = 0u32;
    let container_ok = unsafe {
        GetTokenInformation(
            token,
            TokenIsAppContainer,
            (&mut is_container as *mut u32).cast::<c_void>(),
            std::mem::size_of::<u32>() as u32,
            &mut returned,
        )
    } != 0;
    let mut capabilities = TOKEN_GROUPS::default();
    let capabilities_ok = unsafe {
        GetTokenInformation(
            token,
            TokenCapabilities,
            (&mut capabilities as *mut TOKEN_GROUPS).cast::<c_void>(),
            std::mem::size_of::<TOKEN_GROUPS>() as u32,
            &mut returned,
        )
    } != 0;
    unsafe {
        CloseHandle(token);
    }
    container_ok && is_container == 1 && capabilities_ok && capabilities.GroupCount == 0
}

#[cfg(windows)]
fn probe_port(name: &str) -> u16 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|port| *port != 0)
        .unwrap_or_else(|| std::process::exit(2))
}

#[cfg(target_os = "linux")]
fn native_ip_sockets_are_denied() -> bool {
    let mut denied = true;
    for family in [libc::AF_INET, libc::AF_INET6] {
        let descriptor = unsafe { libc::socket(family, libc::SOCK_STREAM, 0) };
        if descriptor >= 0 {
            unsafe {
                libc::close(descriptor);
            }
            denied = false;
            continue;
        }
        denied &= std::io::Error::last_os_error().raw_os_error() == Some(libc::EACCES);
    }
    denied
}

#[cfg(target_os = "macos")]
fn native_ip_sockets_are_denied() -> bool {
    use std::ffi::c_int;

    unsafe extern "C" {
        fn socket(domain: c_int, kind: c_int, protocol: c_int) -> c_int;
        fn close(descriptor: c_int) -> c_int;
    }
    const AF_INET: c_int = 2;
    const AF_INET6: c_int = 30;
    const SOCK_STREAM: c_int = 1;
    let mut denied = true;
    for family in [AF_INET, AF_INET6] {
        let descriptor = unsafe { socket(family, SOCK_STREAM, 0) };
        if descriptor >= 0 {
            unsafe {
                close(descriptor);
            }
            denied = false;
            continue;
        }
        denied &= matches!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(1) | Some(13) | Some(47)
        );
    }
    denied
}

/// iOS / other targets: binary must compile, but must not report a successful probe.
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn native_ip_sockets_are_denied() -> bool {
    false
}
