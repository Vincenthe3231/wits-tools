//! Restricts the browser-native bridge to the current Windows user's named pipe.

use std::os::windows::process::CommandExt;
use std::{ffi::c_void, fs::File, os::windows::ffi::OsStrExt};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::windows::named_pipe::{NamedPipeServer, ServerOptions},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use windows_sys::Win32::{
    Foundation::{CloseHandle, LocalFree},
    Security::{
        Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            SDDL_REVISION_1,
        },
        GetTokenInformation, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

use super::{Capture, Error, MAX_FRAME, Message, PROTOCOL, Result, failure, reply};

/// Obtain the current user's SID for both the channel name and pipe ACL.
fn identity() -> Result<String> {
    let mut token = std::ptr::null_mut();
    // The token and descriptor buffers are live through each Win32 call and freed below.
    unsafe {
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(Error::CaptureUnavailable);
        }
        let mut length = 0;
        GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut length);
        let mut buffer = vec![0_usize; (length as usize).div_ceil(size_of::<usize>())];
        let read = GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            length,
            &mut length,
        );
        CloseHandle(token);
        if read == 0 {
            return Err(Error::CaptureUnavailable);
        }
        let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        let mut text = std::ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut text) == 0 {
            return Err(Error::CaptureUnavailable);
        }
        let mut length = 0;
        while *text.add(length) != 0 {
            length += 1;
        }
        let sid = String::from_utf16_lossy(std::slice::from_raw_parts(text, length));
        LocalFree(text.cast());
        Ok(sid)
    }
}

fn name(sid: &str) -> String {
    format!(r"\\.\pipe\WitsToolsCapture-{sid}")
}

fn instance(sid: &str, first: bool) -> Result<NamedPipeServer> {
    let sddl = format!("D:P(A;;GA;;;{sid})");
    let wide = std::ffi::OsStr::new(&sddl)
        .encode_wide()
        .chain([0])
        .collect::<Vec<_>>();
    let mut descriptor = std::ptr::null_mut();
    // Apply the same owner-only security descriptor when replenishing listener instances.
    unsafe {
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err(Error::CaptureUnavailable);
        }
        let mut attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let pipe = ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .create_with_security_attributes_raw(
                name(sid),
                (&mut attributes as *mut SECURITY_ATTRIBUTES).cast::<c_void>(),
            );
        LocalFree(descriptor);
        pipe.map_err(|_| Error::CaptureUnavailable)
    }
}

pub(super) fn serve(capture: Capture, stop: CancellationToken) -> Result<()> {
    let sid = identity()?;
    let mut server = instance(&sid, true)?;
    tokio::spawn(async move {
        loop {
            let connected = tokio::select! {
                _ = stop.cancelled() => break,
                result = server.connect() => result,
            };
            if connected.is_err() {
                break;
            }
            let Ok(next) = instance(&sid, false) else {
                break;
            };
            let client = std::mem::replace(&mut server, next);
            let state = capture.clone();
            let cancellation = stop.clone();
            tokio::spawn(async move {
                let id = Uuid::new_v4();
                tokio::select! {
                    _ = cancellation.cancelled() => {},
                    _ = session(&state, client, id) => {},
                }
                state.disconnect(id);
            });
        }
    });
    Ok(())
}

/// Enforce an active connection, tab ID, and version on every native stream message.
async fn session(capture: &Capture, mut pipe: NamedPipeServer, id: Uuid) -> std::io::Result<()> {
    loop {
        let size = pipe.read_u32_le().await? as usize;
        if size == 0 || size > MAX_FRAME {
            return Err(std::io::ErrorKind::InvalidData.into());
        }
        let mut bytes = vec![0; size];
        pipe.read_exact(&mut bytes).await?;
        let message: Message = serde_json::from_slice(&bytes).map_err(std::io::Error::other)?;
        let response = if message.protocol_version != PROTOCOL {
            failure()
        } else {
            match message.kind.as_str() {
                "start" if capture.connect(id, message.tab_id).is_ok() => reply("started"),
                "stream" if message.stream.is_some() => {
                    match capture.native_stream(id, message.tab_id, message.stream.unwrap()) {
                        Ok(()) | Err(Error::UnsupportedMedia) => reply("accepted"),
                        Err(_) => failure(),
                    }
                }
                "stop" => {
                    capture.disconnect(id);
                    reply("stopped")
                }
                _ => failure(),
            }
        };
        let bytes = serde_json::to_vec(&response).map_err(std::io::Error::other)?;
        pipe.write_u32_le(bytes.len() as u32).await?;
        pipe.write_all(&bytes).await?;
        pipe.flush().await?;
        if response.kind == "stopped" {
            return Ok(());
        }
    }
}

pub(super) fn connect() -> Result<File> {
    let sid = identity()?;
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(name(&sid))
        .map_err(|_| Error::CaptureUnavailable)
}

/// Register separate browser manifests with only the WITS Tools extension IDs allowed.
pub(super) fn register() -> Result<()> {
    let executable = std::env::current_exe()
        .map_err(|_| Error::CaptureUnavailable)?
        .with_file_name("wits-tools-capture-host.exe");
    if !executable.is_file() {
        return Err(Error::CaptureUnavailable);
    }
    let root = crate::crawler::data_dir()?.join("native-messaging");
    std::fs::create_dir_all(&root).map_err(|_| Error::File)?;
    let system = std::env::var_os("SystemRoot")
        .map(std::path::PathBuf::from)
        .ok_or(Error::CaptureUnavailable)?;
    let reg = system.join("System32").join("reg.exe");
    for (family, restriction, allowed, registry) in [
        (
            "firefox",
            "allowed_extensions",
            "wits-tools-capture@local",
            r"Mozilla",
        ),
        (
            "zen",
            "allowed_extensions",
            "wits-tools-capture@local",
            r"Zen Browser",
        ),
        (
            "chrome",
            "allowed_origins",
            "chrome-extension://hfcgdoanlppkaencllbjcbbgjckocpoj/",
            r"Google\Chrome",
        ),
        (
            "edge",
            "allowed_origins",
            "chrome-extension://hfcgdoanlppkaencllbjcbbgjckocpoj/",
            r"Microsoft\Edge",
        ),
    ] {
        let mut manifest = serde_json::json!({
            "name": super::HOST_NAME, "description": "WITS Tools selected-tab media capture",
            "path": executable, "type": "stdio"
        });
        manifest[restriction] = serde_json::json!([allowed]);
        let path = root.join(format!("{family}.json"));
        let bytes = serde_json::to_vec_pretty(&manifest).map_err(|_| Error::File)?;
        let mut file = tempfile::NamedTempFile::new_in(&root).map_err(|_| Error::File)?;
        std::io::Write::write_all(&mut file, &bytes).map_err(|_| Error::File)?;
        file.persist(&path).map_err(|_| Error::File)?;
        let key = format!(
            r"HKCU\Software\{registry}\NativeMessagingHosts\{}",
            super::HOST_NAME
        );
        let output = std::process::Command::new(&reg)
            .args(["add", &key, "/ve", "/t", "REG_SZ", "/d"])
            .arg(&path)
            .arg("/f")
            .creation_flags(0x08000000)
            .output()
            .map_err(|_| Error::CaptureUnavailable)?;
        if !output.status.success() {
            return Err(Error::CaptureUnavailable);
        }
    }
    Ok(())
}
