//! Lock-screen feasibility spike (ARCHITECTURE.md §8.5).
//!
//! **Run only on a test VM or secondary PC (decision D3).** It installs a temporary
//! LocalSystem service. When the session locks, the service starts a broker process
//! as SYSTEM in that session on `WinSta0\Winlogon`. The broker answers the three
//! go/no-go questions and logs them to `%ProgramData%\DisplayGroupsSpike\spike.log`:
//!
//! 1. Does it run on the Winlogon desktop while locked?
//! 2. Does `RegisterHotKey` deliver Ctrl+Alt+F11 there?
//! 3. Does `SetDisplayConfig` accept a call from there? (VALIDATE only, so the
//!    displays never change.)
//!
//! The broker registers one hotkey and nothing else: no hooks, no raw input.
//!
//! Usage (elevated prompt): `lockscreen-spike install`, lock with Win+L, press
//! Ctrl+Alt+F11 two or three times, unlock, `lockscreen-spike report`,
//! `lockscreen-spike uninstall`.

#![allow(non_snake_case, clippy::upper_case_acronyms)]

#[cfg(windows)]
fn main() {
    std::process::exit(spike::main());
}

#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
mod spike {
    use std::ffi::c_void;
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::path::PathBuf;
    use std::ptr::{null, null_mut};
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    use dg_engine::ccd::{sdc, Scope};
    use dg_engine::DisplayBackend;
    use dg_win::WinBackend;

    type HANDLE = *mut c_void;
    const SERVICE_NAME: &str = "DisplayGroupsSpike";

    #[repr(C)]
    struct SERVICE_TABLE_ENTRYW {
        name: *mut u16,
        proc_: Option<unsafe extern "system" fn(u32, *mut *mut u16)>,
    }

    #[repr(C)]
    #[derive(Default)]
    struct SERVICE_STATUS {
        dwServiceType: u32,
        dwCurrentState: u32,
        dwControlsAccepted: u32,
        dwWin32ExitCode: u32,
        dwServiceSpecificExitCode: u32,
        dwCheckPoint: u32,
        dwWaitHint: u32,
    }

    #[repr(C)]
    struct WTSSESSION_NOTIFICATION {
        cbSize: u32,
        dwSessionId: u32,
    }

    #[repr(C)]
    struct STARTUPINFOW {
        cb: u32,
        lpReserved: *mut u16,
        lpDesktop: *mut u16,
        lpTitle: *mut u16,
        dwX: u32,
        dwY: u32,
        dwXSize: u32,
        dwYSize: u32,
        dwXCountChars: u32,
        dwYCountChars: u32,
        dwFillAttribute: u32,
        dwFlags: u32,
        wShowWindow: u16,
        cbReserved2: u16,
        lpReserved2: *mut u8,
        hStdInput: HANDLE,
        hStdOutput: HANDLE,
        hStdError: HANDLE,
    }

    #[repr(C)]
    struct PROCESS_INFORMATION {
        hProcess: HANDLE,
        hThread: HANDLE,
        dwProcessId: u32,
        dwThreadId: u32,
    }

    #[repr(C)]
    struct MSG {
        hwnd: HANDLE,
        message: u32,
        wParam: usize,
        lParam: isize,
        time: u32,
        pt: [i32; 2],
        lPrivate: u32,
    }

    const SC_MANAGER_ALL_ACCESS: u32 = 0xF003F;
    const SERVICE_ALL_ACCESS: u32 = 0xF01FF;
    const SERVICE_WIN32_OWN_PROCESS: u32 = 0x10;
    const SERVICE_DEMAND_START: u32 = 3;
    const SERVICE_ERROR_NORMAL: u32 = 1;
    const SERVICE_RUNNING: u32 = 4;
    const SERVICE_STOPPED: u32 = 1;
    const SERVICE_ACCEPT_STOP: u32 = 0x1;
    const SERVICE_ACCEPT_SESSIONCHANGE: u32 = 0x80;
    const SERVICE_CONTROL_STOP: u32 = 0x1;
    const SERVICE_CONTROL_INTERROGATE: u32 = 0x4;
    const SERVICE_CONTROL_SESSIONCHANGE: u32 = 0xE;
    const WTS_SESSION_LOCK: u32 = 0x7;
    const WTS_SESSION_UNLOCK: u32 = 0x8;
    const TOKEN_ALL_ACCESS: u32 = 0xF01FF;
    const MAXIMUM_ALLOWED: u32 = 0x0200_0000;
    const SECURITY_IMPERSONATION: u32 = 2;
    const TOKEN_PRIMARY: u32 = 1;
    const TOKEN_SESSION_ID: u32 = 12;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const INFINITE: u32 = 0xFFFF_FFFF;
    const UOI_NAME: i32 = 2;
    const DESKTOP_READOBJECTS: u32 = 0x0001;
    const MOD_ALT: u32 = 0x1;
    const MOD_CONTROL: u32 = 0x2;
    const MOD_NOREPEAT: u32 = 0x4000;
    const VK_F11: u32 = 0x7A;
    const WM_HOTKEY: u32 = 0x0312;

    #[link(name = "advapi32")]
    extern "system" {
        fn OpenSCManagerW(machine: *const u16, db: *const u16, access: u32) -> HANDLE;
        fn CreateServiceW(
            scm: HANDLE,
            name: *const u16,
            display: *const u16,
            access: u32,
            kind: u32,
            start: u32,
            error: u32,
            path: *const u16,
            group: *const u16,
            tag: *mut u32,
            deps: *const u16,
            account: *const u16,
            password: *const u16,
        ) -> HANDLE;
        fn OpenServiceW(scm: HANDLE, name: *const u16, access: u32) -> HANDLE;
        fn StartServiceW(service: HANDLE, argc: u32, argv: *const *const u16) -> i32;
        fn ControlService(service: HANDLE, control: u32, status: *mut SERVICE_STATUS) -> i32;
        fn DeleteService(service: HANDLE) -> i32;
        fn CloseServiceHandle(h: HANDLE) -> i32;
        fn StartServiceCtrlDispatcherW(table: *const SERVICE_TABLE_ENTRYW) -> i32;
        fn RegisterServiceCtrlHandlerExW(
            name: *const u16,
            handler: unsafe extern "system" fn(u32, u32, *mut c_void, *mut c_void) -> u32,
            context: *mut c_void,
        ) -> HANDLE;
        fn SetServiceStatus(h: HANDLE, status: *const SERVICE_STATUS) -> i32;
        fn OpenProcessToken(process: HANDLE, access: u32, token: *mut HANDLE) -> i32;
        fn DuplicateTokenEx(
            token: HANDLE,
            access: u32,
            attrs: *const c_void,
            level: u32,
            kind: u32,
            new: *mut HANDLE,
        ) -> i32;
        fn SetTokenInformation(token: HANDLE, class: u32, info: *const c_void, len: u32) -> i32;
        fn CreateProcessAsUserW(
            token: HANDLE,
            app: *const u16,
            cmdline: *mut u16,
            pattrs: *const c_void,
            tattrs: *const c_void,
            inherit: i32,
            flags: u32,
            env: *const c_void,
            cwd: *const u16,
            si: *const STARTUPINFOW,
            pi: *mut PROCESS_INFORMATION,
        ) -> i32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateEventW(
            attrs: *const c_void,
            manual: i32,
            initial: i32,
            name: *const u16,
        ) -> HANDLE;
        fn SetEvent(h: HANDLE) -> i32;
        fn WaitForSingleObject(h: HANDLE, ms: u32) -> u32;
        fn CloseHandle(h: HANDLE) -> i32;
        fn TerminateProcess(h: HANDLE, code: u32) -> i32;
        fn GetCurrentProcess() -> HANDLE;
        fn GetCurrentProcessId() -> u32;
        fn GetCurrentThreadId() -> u32;
        fn ProcessIdToSessionId(pid: u32, session: *mut u32) -> i32;
        fn GetLastError() -> u32;
    }

    #[link(name = "user32")]
    extern "system" {
        fn GetThreadDesktop(thread: u32) -> HANDLE;
        fn OpenInputDesktop(flags: u32, inherit: i32, access: u32) -> HANDLE;
        fn CloseDesktop(h: HANDLE) -> i32;
        fn GetUserObjectInformationW(
            obj: HANDLE,
            index: i32,
            info: *mut c_void,
            len: u32,
            needed: *mut u32,
        ) -> i32;
        fn RegisterHotKey(hwnd: HANDLE, id: i32, modifiers: u32, vk: u32) -> i32;
        fn GetMessageW(msg: *mut MSG, hwnd: HANDLE, min: u32, max: u32) -> i32;
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn log_dir() -> PathBuf {
        let base = std::env::var_os("ProgramData")
            .map_or_else(|| PathBuf::from(r"C:\ProgramData"), PathBuf::from);
        base.join("DisplayGroupsSpike")
    }

    fn log(line: &str) {
        let dir = log_dir();
        let _ = std::fs::create_dir_all(&dir);
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if let Ok(mut f) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("spike.log"))
        {
            let _ = writeln!(f, "[{secs}] pid {} {line}", std::process::id());
        }
    }

    pub fn main() -> i32 {
        let arg = std::env::args().nth(1).unwrap_or_default();
        match arg.as_str() {
            "install" => install(),
            "uninstall" => uninstall(),
            "report" => report(),
            "service" => run_service(),
            "broker" => broker(),
            _ => {
                eprintln!("lockscreen-spike install | report | uninstall   (test machines only; elevated prompt)");
                2
            }
        }
    }

    fn install() -> i32 {
        let exe = match std::env::current_exe() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("cannot find this executable: {e}");
                return 1;
            }
        };
        let path = wide(&format!("\"{}\" service", exe.display()));
        let name = wide(SERVICE_NAME);
        let display = wide("DisplayGroups lock-screen spike (temporary)");
        // SAFETY: SCM calls with valid strings; handles are closed.
        unsafe {
            let scm = OpenSCManagerW(null(), null(), SC_MANAGER_ALL_ACCESS);
            if scm.is_null() {
                eprintln!(
                    "Cannot open the service manager ({}). Run from an elevated prompt.",
                    GetLastError()
                );
                return 1;
            }
            let svc = CreateServiceW(
                scm,
                name.as_ptr(),
                display.as_ptr(),
                SERVICE_ALL_ACCESS,
                SERVICE_WIN32_OWN_PROCESS,
                SERVICE_DEMAND_START,
                SERVICE_ERROR_NORMAL,
                path.as_ptr(),
                null(),
                null_mut(),
                null(),
                null(), // LocalSystem
                null(),
            );
            if svc.is_null() {
                eprintln!("CreateService failed ({}).", GetLastError());
                CloseServiceHandle(scm);
                return 1;
            }
            let started = StartServiceW(svc, 0, null()) != 0;
            CloseServiceHandle(svc);
            CloseServiceHandle(scm);
            if !started {
                eprintln!(
                    "The service was created but did not start ({}).",
                    GetLastError()
                );
                return 1;
            }
        }
        println!("Spike service installed and running.");
        println!("Now: lock with Win+L, wait 3 s, press Ctrl+Alt+F11 two or three times, unlock,");
        println!("then run:  lockscreen-spike report   and finally   lockscreen-spike uninstall");
        0
    }

    fn uninstall() -> i32 {
        let name = wide(SERVICE_NAME);
        // SAFETY: SCM calls with valid strings; handles are closed.
        unsafe {
            let scm = OpenSCManagerW(null(), null(), SC_MANAGER_ALL_ACCESS);
            if scm.is_null() {
                eprintln!(
                    "Cannot open the service manager ({}). Run from an elevated prompt.",
                    GetLastError()
                );
                return 1;
            }
            let svc = OpenServiceW(scm, name.as_ptr(), SERVICE_ALL_ACCESS);
            if svc.is_null() {
                eprintln!("The spike service is not installed.");
                CloseServiceHandle(scm);
                return 0;
            }
            let mut status = SERVICE_STATUS::default();
            ControlService(svc, SERVICE_CONTROL_STOP, &mut status);
            let deleted = DeleteService(svc) != 0;
            CloseServiceHandle(svc);
            CloseServiceHandle(scm);
            if !deleted {
                eprintln!("DeleteService failed ({}).", GetLastError());
                return 1;
            }
        }
        println!(
            "Spike service removed. The log stays in {}.",
            log_dir().display()
        );
        0
    }

    fn report() -> i32 {
        let path = log_dir().join("spike.log");
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                print!("{text}");
                let ok = |needle: &str| text.contains(needle);
                println!("\n--- verdict ---");
                println!(
                    "1. broker on Winlogon desktop while locked: {}",
                    yes(ok("thread desktop=Winlogon"))
                );
                println!(
                    "2. RegisterHotKey delivered Ctrl+Alt+F11:   {}",
                    yes(ok("HOTKEY RECEIVED"))
                );
                println!(
                    "3. SetDisplayConfig accepted (VALIDATE=0):   {}",
                    yes(ok("SetDisplayConfig VALIDATE -> 0"))
                );
                0
            }
            Err(e) => {
                eprintln!("{}: {e}", path.display());
                1
            }
        }
    }

    fn yes(b: bool) -> &'static str {
        if b {
            "YES"
        } else {
            "no (see log)"
        }
    }

    // ----- service -----

    static STATUS_HANDLE: Mutex<usize> = Mutex::new(0);
    static STOP_EVENT: Mutex<usize> = Mutex::new(0);
    static BROKER: Mutex<usize> = Mutex::new(0);

    fn run_service() -> i32 {
        let mut name = wide(SERVICE_NAME);
        let table = [
            SERVICE_TABLE_ENTRYW {
                name: name.as_mut_ptr(),
                proc_: Some(service_main),
            },
            SERVICE_TABLE_ENTRYW {
                name: null_mut(),
                proc_: None,
            },
        ];
        // SAFETY: the table outlives the dispatcher, which returns when the service stops.
        if unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) } == 0 {
            eprintln!("This command is started by the service manager, not by hand.");
            return 1;
        }
        0
    }

    fn set_state(state: u32) {
        let h = *STATUS_HANDLE.lock().unwrap_or_else(|e| e.into_inner());
        let status = SERVICE_STATUS {
            dwServiceType: SERVICE_WIN32_OWN_PROCESS,
            dwCurrentState: state,
            dwControlsAccepted: if state == SERVICE_RUNNING {
                SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SESSIONCHANGE
            } else {
                0
            },
            ..Default::default()
        };
        // SAFETY: handle from RegisterServiceCtrlHandlerExW.
        unsafe { SetServiceStatus(h as HANDLE, &status) };
    }

    unsafe extern "system" fn service_main(_argc: u32, _argv: *mut *mut u16) {
        let name = wide(SERVICE_NAME);
        // SAFETY: registering our handler and creating the stop event.
        unsafe {
            let h = RegisterServiceCtrlHandlerExW(name.as_ptr(), handler, null_mut());
            *STATUS_HANDLE.lock().unwrap_or_else(|e| e.into_inner()) = h as usize;
            let stop = CreateEventW(null(), 1, 0, null());
            *STOP_EVENT.lock().unwrap_or_else(|e| e.into_inner()) = stop as usize;
            set_state(SERVICE_RUNNING);
            log("service started");
            WaitForSingleObject(stop, INFINITE);
            kill_broker();
            log("service stopping");
            set_state(SERVICE_STOPPED);
        }
    }

    unsafe extern "system" fn handler(
        control: u32,
        event: u32,
        data: *mut c_void,
        _ctx: *mut c_void,
    ) -> u32 {
        match control {
            SERVICE_CONTROL_STOP => {
                let stop = *STOP_EVENT.lock().unwrap_or_else(|e| e.into_inner());
                // SAFETY: event created in service_main.
                unsafe { SetEvent(stop as HANDLE) };
            }
            SERVICE_CONTROL_SESSIONCHANGE => {
                // SAFETY: for session changes, data points at a WTSSESSION_NOTIFICATION.
                let session = unsafe { (*(data as *const WTSSESSION_NOTIFICATION)).dwSessionId };
                match event {
                    WTS_SESSION_LOCK => {
                        log(&format!("session {session} locked: starting broker"));
                        spawn_broker(session);
                    }
                    WTS_SESSION_UNLOCK => {
                        log(&format!("session {session} unlocked: stopping broker"));
                        kill_broker();
                    }
                    _ => {}
                }
            }
            SERVICE_CONTROL_INTERROGATE => {}
            _ => {}
        }
        0
    }

    fn spawn_broker(session: u32) {
        let Ok(exe) = std::env::current_exe() else {
            log("cannot find own executable");
            return;
        };
        let app = wide(&exe.to_string_lossy());
        let mut cmdline = wide(&format!("\"{}\" broker", exe.display()));
        let mut desktop = wide("winsta0\\Winlogon");
        // SAFETY: token duplication and process creation with valid, NUL-terminated
        // buffers; every handle is closed or stored.
        unsafe {
            let mut token = null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_ALL_ACCESS, &mut token) == 0 {
                log(&format!("OpenProcessToken failed ({})", GetLastError()));
                return;
            }
            let mut primary = null_mut();
            let dup = DuplicateTokenEx(
                token,
                MAXIMUM_ALLOWED,
                null(),
                SECURITY_IMPERSONATION,
                TOKEN_PRIMARY,
                &mut primary,
            );
            CloseHandle(token);
            if dup == 0 {
                log(&format!("DuplicateTokenEx failed ({})", GetLastError()));
                return;
            }
            if SetTokenInformation(
                primary,
                TOKEN_SESSION_ID,
                (&session as *const u32).cast(),
                4,
            ) == 0
            {
                log(&format!(
                    "SetTokenInformation(TokenSessionId) failed ({})",
                    GetLastError()
                ));
                CloseHandle(primary);
                return;
            }
            let si = STARTUPINFOW {
                cb: std::mem::size_of::<STARTUPINFOW>() as u32,
                lpReserved: null_mut(),
                lpDesktop: desktop.as_mut_ptr(),
                lpTitle: null_mut(),
                dwX: 0,
                dwY: 0,
                dwXSize: 0,
                dwYSize: 0,
                dwXCountChars: 0,
                dwYCountChars: 0,
                dwFillAttribute: 0,
                dwFlags: 0,
                wShowWindow: 0,
                cbReserved2: 0,
                lpReserved2: null_mut(),
                hStdInput: null_mut(),
                hStdOutput: null_mut(),
                hStdError: null_mut(),
            };
            let mut pi = PROCESS_INFORMATION {
                hProcess: null_mut(),
                hThread: null_mut(),
                dwProcessId: 0,
                dwThreadId: 0,
            };
            let ok = CreateProcessAsUserW(
                primary,
                app.as_ptr(),
                cmdline.as_mut_ptr(),
                null(),
                null(),
                0,
                CREATE_NO_WINDOW,
                null(),
                null(),
                &si,
                &mut pi,
            );
            CloseHandle(primary);
            if ok == 0 {
                log(&format!("CreateProcessAsUserW failed ({})", GetLastError()));
                return;
            }
            CloseHandle(pi.hThread);
            log(&format!("broker started, pid {}", pi.dwProcessId));
            kill_broker();
            *BROKER.lock().unwrap_or_else(|e| e.into_inner()) = pi.hProcess as usize;
        }
    }

    fn kill_broker() {
        let mut slot = BROKER.lock().unwrap_or_else(|e| e.into_inner());
        if *slot != 0 {
            // SAFETY: process handle we own.
            unsafe {
                TerminateProcess(*slot as HANDLE, 0);
                CloseHandle(*slot as HANDLE);
            }
            *slot = 0;
        }
    }

    // ----- broker (SYSTEM, user session, Winlogon desktop) -----

    fn desktop_name(h: HANDLE) -> String {
        let mut buf = [0u16; 128];
        let mut needed = 0;
        // SAFETY: fixed buffer with its size in bytes.
        let ok = unsafe {
            GetUserObjectInformationW(h, UOI_NAME, buf.as_mut_ptr().cast(), 256, &mut needed)
        };
        if ok == 0 {
            return format!("? (error {})", unsafe { GetLastError() });
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..end])
    }

    fn validate_display_config(label: &str) {
        let backend = WinBackend::new();
        match backend.query(Scope::Active) {
            Ok(topo) => {
                let paths: Vec<_> = topo.active_paths().copied().collect();
                let code = backend.set_config(
                    &paths,
                    &topo.modes,
                    sdc::USE_SUPPLIED_DISPLAY_CONFIG | sdc::VALIDATE,
                );
                log(&format!("{label}: QueryDisplayConfig ok ({} active), SetDisplayConfig VALIDATE -> {code}", paths.len()));
            }
            Err(e) => log(&format!("{label}: QueryDisplayConfig failed: {e}")),
        }
    }

    fn broker() -> i32 {
        // SAFETY: read-only queries about this process, thread and desktops.
        unsafe {
            let mut session = 0;
            ProcessIdToSessionId(GetCurrentProcessId(), &mut session);
            let thread_desktop = desktop_name(GetThreadDesktop(GetCurrentThreadId()));
            let input = OpenInputDesktop(0, 0, DESKTOP_READOBJECTS);
            let input_desktop = if input.is_null() {
                format!("? (OpenInputDesktop error {})", GetLastError())
            } else {
                let n = desktop_name(input);
                CloseDesktop(input);
                n
            };
            log(&format!("broker up: session {session}, thread desktop={thread_desktop}, input desktop={input_desktop}"));
            validate_display_config("at start");

            if RegisterHotKey(null_mut(), 1, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, VK_F11) == 0 {
                log(&format!(
                    "RegisterHotKey(Ctrl+Alt+F11) FAILED ({})",
                    GetLastError()
                ));
            } else {
                log("RegisterHotKey(Ctrl+Alt+F11) ok; waiting");
            }
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
                if msg.message == WM_HOTKEY {
                    log("HOTKEY RECEIVED (Ctrl+Alt+F11)");
                    validate_display_config("on hotkey");
                }
            }
        }
        0
    }
}
