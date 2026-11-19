//! Named pipes with an explicit DACL (SDDL), remote clients rejected, and caller
//! identification. Clients connect at `SECURITY_IDENTIFICATION` level, so the server
//! can read who they are but can never act as them.

#![allow(non_snake_case, clippy::upper_case_acronyms)]

use std::ffi::c_void;
use std::io::{self, Read, Write};
use std::ptr::{null, null_mut};

type HANDLE = *mut c_void;

#[repr(C)]
struct SECURITY_ATTRIBUTES {
    nLength: u32,
    lpSecurityDescriptor: *mut c_void,
    bInheritHandle: i32,
}

#[repr(C)]
struct SID_AND_ATTRIBUTES {
    Sid: *mut c_void,
    Attributes: u32,
}

const PIPE_ACCESS_DUPLEX: u32 = 0x3;
const PIPE_TYPE_BYTE: u32 = 0x0;
const PIPE_REJECT_REMOTE_CLIENTS: u32 = 0x8;
const PIPE_UNLIMITED_INSTANCES: u32 = 255;
const BUFFER: u32 = 64 * 1024;
const ERROR_PIPE_CONNECTED: i32 = 535;
const GENERIC_READ: u32 = 0x8000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;
const OPEN_EXISTING: u32 = 3;
const SECURITY_SQOS_PRESENT: u32 = 0x0010_0000;
const SECURITY_IDENTIFICATION: u32 = 0x0001_0000;
const TOKEN_QUERY: u32 = 0x0008;
const TOKEN_USER_CLASS: u32 = 1;
const SDDL_REVISION_1: u32 = 1;

#[link(name = "kernel32")]
extern "system" {
    fn CreateNamedPipeW(
        name: *const u16,
        open_mode: u32,
        pipe_mode: u32,
        max_instances: u32,
        out_buffer: u32,
        in_buffer: u32,
        timeout: u32,
        sa: *const SECURITY_ATTRIBUTES,
    ) -> HANDLE;
    fn ConnectNamedPipe(pipe: HANDLE, overlapped: *mut c_void) -> i32;
    fn DisconnectNamedPipe(pipe: HANDLE) -> i32;
    fn CreateFileW(
        name: *const u16,
        access: u32,
        share: u32,
        sa: *const SECURITY_ATTRIBUTES,
        disposition: u32,
        flags: u32,
        template: HANDLE,
    ) -> HANDLE;
    fn ReadFile(h: HANDLE, buf: *mut u8, len: u32, read: *mut u32, overlapped: *mut c_void) -> i32;
    fn WriteFile(
        h: HANDLE,
        buf: *const u8,
        len: u32,
        written: *mut u32,
        overlapped: *mut c_void,
    ) -> i32;
    fn FlushFileBuffers(h: HANDLE) -> i32;
    fn CloseHandle(h: HANDLE) -> i32;
    fn GetNamedPipeClientSessionId(pipe: HANDLE, session: *mut u32) -> i32;
    fn GetCurrentThread() -> HANDLE;
    fn GetCurrentProcess() -> HANDLE;
    fn LocalFree(mem: *mut c_void) -> *mut c_void;
}

#[link(name = "advapi32")]
extern "system" {
    fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
        sddl: *const u16,
        revision: u32,
        descriptor: *mut *mut c_void,
        size: *mut u32,
    ) -> i32;
    fn ImpersonateNamedPipeClient(pipe: HANDLE) -> i32;
    fn RevertToSelf() -> i32;
    fn OpenThreadToken(thread: HANDLE, access: u32, as_self: i32, token: *mut HANDLE) -> i32;
    fn OpenProcessToken(process: HANDLE, access: u32, token: *mut HANDLE) -> i32;
    fn GetTokenInformation(
        token: HANDLE,
        class: u32,
        info: *mut c_void,
        len: u32,
        needed: *mut u32,
    ) -> i32;
    fn ConvertSidToStringSidW(sid: *mut c_void, out: *mut *mut u16) -> i32;
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn last_error() -> io::Error {
    io::Error::last_os_error()
}

/// Accepts connections on a named pipe protected by `sddl`.
pub struct PipeServer {
    name: Vec<u16>,
    descriptor: *mut c_void,
}

// SAFETY: the descriptor is immutable after creation and only freed in Drop.
unsafe impl Send for PipeServer {}

impl PipeServer {
    pub fn new(name: &str, sddl: &str) -> io::Result<Self> {
        let s = wide(sddl);
        let mut descriptor = null_mut();
        // SAFETY: valid SDDL string; the descriptor is freed with LocalFree in Drop.
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                s.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                null_mut(),
            )
        };
        if ok == 0 {
            return Err(last_error());
        }
        Ok(Self {
            name: wide(name),
            descriptor,
        })
    }

    /// Creates a pipe instance and waits for one client.
    pub fn accept(&self) -> io::Result<PipeConnection> {
        let sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.descriptor,
            bInheritHandle: 0,
        };
        // SAFETY: valid name and security attributes.
        let handle = unsafe {
            CreateNamedPipeW(
                self.name.as_ptr(),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                BUFFER,
                BUFFER,
                0,
                &sa,
            )
        };
        if handle as isize == -1 {
            return Err(last_error());
        }
        let conn = PipeConnection {
            handle,
            server: true,
        };
        // SAFETY: blocking connect on our own handle.
        if unsafe { ConnectNamedPipe(handle, null_mut()) } == 0 {
            let e = last_error();
            if e.raw_os_error() != Some(ERROR_PIPE_CONNECTED) {
                return Err(e);
            }
        }
        Ok(conn)
    }
}

impl Drop for PipeServer {
    fn drop(&mut self) {
        // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW.
        unsafe { LocalFree(self.descriptor) };
    }
}

/// One end of a connected pipe.
pub struct PipeConnection {
    handle: HANDLE,
    server: bool,
}

// SAFETY: a pipe handle can be used from any thread.
unsafe impl Send for PipeConnection {}

impl PipeConnection {
    /// Connects to a server, allowing it only to *identify* this process.
    pub fn connect(name: &str) -> io::Result<Self> {
        let n = wide(name);
        // SAFETY: valid name; no template.
        let handle = unsafe {
            CreateFileW(
                n.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                null(),
                OPEN_EXISTING,
                SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                null_mut(),
            )
        };
        if handle as isize == -1 {
            return Err(last_error());
        }
        Ok(Self {
            handle,
            server: false,
        })
    }

    /// Terminal-services session of the connected client.
    pub fn client_session_id(&self) -> io::Result<u32> {
        let mut session = 0;
        // SAFETY: out-pointer to a local.
        if unsafe { GetNamedPipeClientSessionId(self.handle, &mut session) } == 0 {
            return Err(last_error());
        }
        Ok(session)
    }

    /// The client's user SID (e.g. `S-1-5-21-…`), read through identification-level
    /// impersonation that is reverted before returning.
    pub fn client_sid(&self) -> io::Result<String> {
        // SAFETY: impersonation is always reverted below, also on error.
        unsafe {
            if ImpersonateNamedPipeClient(self.handle) == 0 {
                return Err(last_error());
            }
            let mut token = null_mut();
            let opened = OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut token);
            let open_error = last_error();
            RevertToSelf();
            if opened == 0 {
                return Err(open_error);
            }
            let sid = token_user_sid(token);
            CloseHandle(token);
            sid
        }
    }
}

/// SID of the user running this process.
pub fn current_user_sid() -> io::Result<String> {
    // SAFETY: query-only access to our own token.
    unsafe {
        let mut token = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(last_error());
        }
        let sid = token_user_sid(token);
        CloseHandle(token);
        sid
    }
}

/// # Safety
/// `token` must be a valid token handle with `TOKEN_QUERY` access.
unsafe fn token_user_sid(token: HANDLE) -> io::Result<String> {
    // SAFETY: two-call pattern with a buffer of the size Windows asked for.
    unsafe {
        let mut needed = 0u32;
        GetTokenInformation(token, TOKEN_USER_CLASS, null_mut(), 0, &mut needed);
        if needed == 0 {
            return Err(last_error());
        }
        let mut buf = vec![0u64; (needed as usize).div_ceil(8)];
        if GetTokenInformation(
            token,
            TOKEN_USER_CLASS,
            buf.as_mut_ptr().cast(),
            needed,
            &mut needed,
        ) == 0
        {
            return Err(last_error());
        }
        let user = &*(buf.as_ptr() as *const SID_AND_ATTRIBUTES);
        let mut text: *mut u16 = null_mut();
        if ConvertSidToStringSidW(user.Sid, &mut text) == 0 {
            return Err(last_error());
        }
        let len = (0..).take_while(|&i| *text.add(i) != 0).count();
        let sid = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
        LocalFree(text.cast());
        Ok(sid)
    }
}

impl Read for PipeConnection {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut read = 0u32;
        let len = u32::try_from(buf.len()).unwrap_or(u32::MAX);
        // SAFETY: buffer of `len` bytes.
        if unsafe { ReadFile(self.handle, buf.as_mut_ptr(), len, &mut read, null_mut()) } == 0 {
            return Err(last_error());
        }
        Ok(read as usize)
    }
}

impl Write for PipeConnection {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut written = 0u32;
        let len = u32::try_from(buf.len()).unwrap_or(u32::MAX);
        // SAFETY: buffer of `len` bytes.
        if unsafe { WriteFile(self.handle, buf.as_ptr(), len, &mut written, null_mut()) } == 0 {
            return Err(last_error());
        }
        Ok(written as usize)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for PipeConnection {
    fn drop(&mut self) {
        // SAFETY: our own handle; the server flushes so the client can read the reply.
        unsafe {
            if self.server {
                FlushFileBuffers(self.handle);
                DisconnectNamedPipe(self.handle);
            }
            CloseHandle(self.handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framing::{read_message, write_message};
    use crate::protocol::{Request, Response, PROTOCOL_VERSION, SERVICE_PIPE_SDDL};

    #[test]
    fn round_trip_with_caller_identity() {
        let name = format!(r"\\.\pipe\DisplayGroups.Test.{}", std::process::id());
        let server = PipeServer::new(&name, SERVICE_PIPE_SDDL).unwrap();
        let handle = std::thread::spawn(move || {
            let mut conn = server.accept().unwrap();
            let session = conn.client_session_id().unwrap();
            let sid = conn.client_sid().unwrap();
            let request: Request = read_message(&mut conn).unwrap();
            write_message(
                &mut conn,
                &Response::Hello {
                    version: PROTOCOL_VERSION,
                },
            )
            .unwrap();
            (session, sid, request)
        });

        let mut client = loop {
            match PipeConnection::connect(&name) {
                Ok(c) => break c,
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(20)),
            }
        };
        write_message(
            &mut client,
            &Request::Hello {
                version: PROTOCOL_VERSION,
            },
        )
        .unwrap();
        let reply: Response = read_message(&mut client).unwrap();
        assert_eq!(
            reply,
            Response::Hello {
                version: PROTOCOL_VERSION
            }
        );

        let (session, sid, request) = handle.join().unwrap();
        assert_eq!(
            request,
            Request::Hello {
                version: PROTOCOL_VERSION
            }
        );
        assert_eq!(sid, current_user_sid().unwrap());
        assert!(sid.starts_with("S-1-5-"));
        let mut own_session = 0;
        #[link(name = "kernel32")]
        extern "system" {
            fn ProcessIdToSessionId(pid: u32, session: *mut u32) -> i32;
        }
        // SAFETY: out-pointer to a local.
        unsafe { ProcessIdToSessionId(std::process::id(), &mut own_session) };
        assert_eq!(session, own_session);
    }

    #[test]
    fn invalid_sddl_is_refused() {
        assert!(PipeServer::new(r"\\.\pipe\x", "not sddl").is_err());
    }
}
