//! Windows only: UIPI blocks `SendInput` into windows of higher integrity
//! (Task Manager, elevated apps), so users can opt into running elevated.
//! One UAC prompt copies this exe to `%ProgramFiles%\AgentsPads\agentspads.exe`
//! (ACL inherited from Program Files) and registers an on-demand "highest
//! privileges" task that runs that copy. Later launches, including login
//! autostart, re-exec through the task with no prompt. Task XML is written
//! only in that directory and removed after `schtasks /Create`.
//!
//! The elevated process must not create or modify anything under the user
//! profile, Startup folder, or `AGENTPAD_DATA_DIR`. Admin mode is the marker
//! `%ProgramFiles%\AgentsPads\run_as_admin.txt`, written by the elevated
//! installer and removed with that directory. Theme, the guide flag, and logs
//! live in `state\` beside it and keep the inherited Program Files ACL, so
//! ordinary users can still open logs. Login autostart is applied by the
//! unelevated starter before re-exec, using that process's own executable.

#[cfg(any(windows, test))]
const TASK_NAME: &str = "AgentsPads Elevated";
#[cfg(windows)]
const MARKER_FILE: &str = "run_as_admin.txt";
/// Login autostart in admin mode is a second task with a logon trigger; this
/// file mirrors whether it is enabled so the settings page can show it.
#[cfg(any(windows, test))]
const AUTOSTART_TASK_NAME: &str = "AgentsPads Elevated Autostart";
#[cfg(windows)]
const AUTOSTART_MARKER_FILE: &str = "autostart.txt";
#[cfg(any(windows, test))]
const AUTOSTART_FLAG: &str = "--admin-autostart";
/// Login is busy; this app can wait behind everything else.
#[cfg(any(windows, test))]
const LOGON_DELAY: &str = "PT30S";
#[cfg(windows)]
const STATE_DIR: &str = "state";
#[cfg(windows)]
const TASK_XML_NAME: &str = "agentspads-task.xml";
#[cfg(any(windows, test))]
const SECRET_DIR: &str = "secret";
const RELAUNCH_FLAG: &str = "--elevated";
/// Not `--after-update`: that flag means a finished update (delete `*.old`)
/// and never waits for the parent process.
#[cfg(any(windows, test))]
const HANDOFF_FLAG: &str = "--handoff";
#[cfg(windows)]
const HANDOFF_WAIT: std::time::Duration = std::time::Duration::from_secs(10);
#[cfg(any(windows, test))]
const STARTUP_ERROR_FILE: &str = "startup_error.txt";
#[cfg(any(windows, test))]
const STARTUP_ERROR_MAX: usize = 1024;
/// Ordinary readers ignore anything older than a few minutes.
#[cfg(any(windows, test))]
const STARTUP_ERROR_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(180);
#[cfg(windows)]
static HANDOFF_FAILURE: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
#[cfg(any(windows, test))]
const INSTALL_FLAG: &str = "--install-admin-task";
#[cfg(any(windows, test))]
const REMOVE_FLAG: &str = "--remove-admin-task";

/// The elevated child may start before the parent has released the port.
pub fn relaunched() -> bool {
    std::env::args().any(|a| a == RELAUNCH_FLAG)
}

/// `--handoff <pid>` from the settings switch: block until that process exits
/// (or the timeout elapses) so it can release port 9618 before we relaunch.
pub fn wait_for_handoff_parent() -> Result<(), &'static str> {
    #[cfg(windows)]
    {
        if let Some(pid) = handoff_parent_pid(std::env::args()) {
            return wait_for_pid(pid, HANDOFF_WAIT);
        }
    }
    Ok(())
}

pub fn invalid_elevation_attempt() -> bool {
    cfg!(windows) && reject_ordinary_attempt(relaunched(), is_elevated())
}

fn reject_ordinary_attempt(attempt: bool, elevated: bool) -> bool {
    attempt && !elevated
}

/// Call before binding the port; true means this process should exit.
pub fn relaunch_if_needed() -> bool {
    #[cfg(windows)]
    {
        if !enabled() || is_elevated() {
            return false;
        }
        let attempt = std::time::SystemTime::now();
        let task_started = run_task();
        if task_started && wait_for_ready(None) {
            return true;
        }
        let launched = launch_protected_elevated();
        if let Ok(process) = &launched {
            if wait_for_ready(Some(process)) {
                return true;
            }
        }
        let detail = recent_startup_error(attempt);
        let reason = explain_handoff_failure(
            task_started,
            matches!(launched, Err(LaunchError::Cancelled)),
            detail.as_deref(),
        );
        crate::logutil::write("admin relaunch failed");
        note_handoff_failure(&reason);
        false
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Failure text for the settings row. Empty on macOS and after a clean handoff.
#[cfg(windows)]
pub fn handoff_failure() -> Option<String> {
    HANDOFF_FAILURE.lock().unwrap().clone()
}

#[cfg(windows)]
pub fn note_handoff_failure(reason: &str) {
    *HANDOFF_FAILURE.lock().unwrap() = Some(reason.to_string());
}

/// Spawn the current exe as an ordinary process, then the caller should exit.
/// The child waits for this pid and runs `relaunch_if_needed`.
#[cfg(windows)]
pub fn spawn_unelevated_handoff() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    std::process::Command::new(exe)
        .arg(HANDOFF_FLAG)
        .arg(std::process::id().to_string())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_ok()
}

/// Elevated process only. One short line under Program Files `state\`.
/// Ordinary and non-Windows calls do not touch the user profile.
pub fn note_startup_failure(kind: &str, detail: &str) {
    #[cfg(windows)]
    {
        if !is_elevated() {
            return;
        }
        let Some(dir) = protected_state_dir() else {
            return;
        };
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let text = startup_failure_text(kind, detail);
        let _ = std::fs::write(
            dir.join(STARTUP_ERROR_FILE),
            startup_error_line(&text, secs),
        );
    }
    #[cfg(not(windows))]
    {
        let _ = (kind, detail);
    }
}

/// `--install-admin-task` / `--remove-admin-task`: do the work and exit.
pub fn exit_if_admin_maintenance() {
    #[cfg(windows)]
    {
        let code = match early_action(std::env::args()) {
            EarlyAction::None => return,
            EarlyAction::Install(user, autostart) => {
                if install_protected(&user, autostart) {
                    0
                } else {
                    1
                }
            }
            EarlyAction::Remove => {
                remove_protected();
                0
            }
            EarlyAction::Bad => 1,
        };
        std::process::exit(code);
    }
}

/// Marker file plus the protected copy. Both live under Program Files, so an
/// ordinary process can read them but cannot turn admin mode on by writing a
/// user-profile file. A missing scheduled task is noticed when the relaunch
/// itself fails; this check does not spawn `schtasks`.
#[cfg(windows)]
pub fn enabled() -> bool {
    let Some((dir, exe)) = protected_target() else {
        return false;
    };
    admin_mode_enabled(marker_is_set(&dir), exe.is_file())
}

pub fn is_elevated() -> bool {
    // Tests always run as "not elevated": CI Windows runners are administrators, and
    // an elevated test process would take the Program Files code paths.
    #[cfg(all(windows, not(test)))]
    {
        unsafe { windows::Win32::UI::Shell::IsUserAnAdmin().as_bool() }
    }
    #[cfg(any(not(windows), test))]
    {
        false
    }
}

/// One UAC prompt, unless this process is already elevated.
#[cfg(windows)]
pub fn install_task(autostart: bool) -> Result<(), &'static str> {
    let (Ok(exe), Some(user)) = (std::env::current_exe(), windows_user()) else {
        return Err("无法确定当前程序或用户");
    };
    let mut args = vec![INSTALL_FLAG, user.as_str()];
    if autostart {
        args.push(AUTOSTART_FLAG);
    }
    shell_exec_wait(exe.as_os_str(), &join_windows_args(&args))
}

/// One UAC prompt, unless this process is already elevated. Failure is ignored.
#[cfg(windows)]
pub fn remove_task() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let _ = shell_exec_wait(exe.as_os_str(), REMOVE_FLAG);
}

/// Identity file of the elevated instance. Its pairing secret must not be
/// readable by ordinary same-user processes, which could otherwise authenticate
/// and inject input into elevated windows through this instance.
///
/// Lives under `%ProgramFiles%\AgentsPads\secret`: ordinary users can neither
/// create nor replace entries in that tree, so nobody can pre-create it or turn
/// it into a link. The directory and file are then re-owned by Administrators
/// (the creator may be the user SID, which a non-elevated token of the same user
/// would match) and the DACL is cut to Administrators/SYSTEM. The empty file is
/// locked before the secret is written. Any failure is an error; never fall
/// back to the ordinary instance's key. Disabling admin mode deletes the tree.
#[cfg(windows)]
pub fn admin_identity_path() -> std::io::Result<std::path::PathBuf> {
    let err = |msg: &str| std::io::Error::other(msg.to_string());
    if !is_elevated() {
        return Err(err("not elevated"));
    }
    let (dir, _) = protected_target().ok_or_else(|| err("no program files"))?;
    let dir = dir.join(SECRET_DIR);
    std::fs::create_dir_all(&dir)?;
    let file = dir.join("identity.json");
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)?;
    let dir_text = dir.to_str().ok_or_else(|| err("bad path"))?;
    for args in lock_commands(dir_text) {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        if !system32("icacls.exe", &args) {
            return Err(err("icacls failed"));
        }
    }
    Ok(file)
}

/// Well-known SIDs, so localized group names cannot break it.
#[cfg(any(windows, test))]
fn lock_commands(dir: &str) -> [Vec<String>; 2] {
    let owner = vec![
        dir.to_string(),
        "/setowner".into(),
        "*S-1-5-32-544".into(),
        "/T".into(),
    ];
    let acl = vec![
        dir.to_string(),
        "/inheritance:r".into(),
        "/grant:r".into(),
        "*S-1-5-32-544:(OI)(CI)F".into(),
        "*S-1-5-18:(OI)(CI)F".into(),
        "/T".into(),
    ];
    [owner, acl]
}

/// Running an existing task needs no elevation and shows no prompt.
#[cfg(windows)]
pub fn run_task() -> bool {
    schtasks(&["/Run", "/TN", TASK_NAME])
}

#[cfg(windows)]
fn flag_is_true(path: &std::path::Path) -> bool {
    std::fs::read_to_string(path).is_ok_and(|value| value.trim() == "true")
}

#[cfg(windows)]
fn marker_is_set(dir: &std::path::Path) -> bool {
    flag_is_true(&dir.join(MARKER_FILE))
}

#[cfg(windows)]
fn write_flag(dir: &std::path::Path, file: &str, on: bool) -> bool {
    std::fs::write(dir.join(file), on.to_string()).is_ok()
}

/// Admin-mode login autostart state, from the protected directory.
#[cfg(windows)]
pub fn autostart_enabled() -> bool {
    protected_target().is_some_and(|(dir, _)| flag_is_true(&dir.join(AUTOSTART_MARKER_FILE)))
}

/// Enable or disable the logon-trigger task. Only the elevated instance may do
/// this (no UAC prompt, and ordinary processes cannot change the task); nothing
/// in the user profile is touched.
#[cfg(windows)]
pub fn set_autostart(on: bool) -> bool {
    let Some((dir, _)) = protected_target() else {
        return false;
    };
    is_elevated()
        && schtasks(&[
            "/Change",
            "/TN",
            AUTOSTART_TASK_NAME,
            if on { "/ENABLE" } else { "/DISABLE" },
        ])
        && write_flag(&dir, AUTOSTART_MARKER_FILE, on)
}

/// Runtime files for the elevated process (theme, guide flag, logs). Not locked
/// down like `secret\`: callers create it and inherit the Program Files ACL.
#[cfg(windows)]
pub(crate) fn protected_state_dir() -> Option<std::path::PathBuf> {
    let path = protected_target()?.0.join(STATE_DIR);
    no_reparse_ancestors(&path).then_some(path)
}

#[cfg(windows)]
fn install_protected(user: &str, autostart: bool) -> bool {
    if !is_elevated() {
        return false;
    }
    let (Ok(src), Some((dir, dest))) = (std::env::current_exe(), protected_target()) else {
        return false;
    };
    if std::fs::create_dir_all(&dir).is_err() || install_copy(&src, &dest).is_err() {
        return false;
    }
    let Some(command) = dest.to_str() else {
        return false;
    };
    let xml_path = dir.join(TASK_XML_NAME);
    let create = |name: &str, xml: String| {
        write_utf16(&xml_path, &xml).is_ok()
            && xml_path
                .to_str()
                .is_some_and(|path| schtasks(&["/Create", "/TN", name, "/XML", path, "/F"]))
    };
    let ok = create(TASK_NAME, task_xml(command, Some(user), None))
        && create(
            AUTOSTART_TASK_NAME,
            task_xml(command, Some(user), Some(autostart)),
        );
    let _ = std::fs::remove_file(xml_path);
    // Markers only after the tasks exist, so a failed install cannot arm admin mode
    // against a leftover task. Not secrets: they inherit the Program Files ACL.
    ok && write_flag(&dir, AUTOSTART_MARKER_FILE, autostart) && write_flag(&dir, MARKER_FILE, true)
}

#[cfg(windows)]
fn remove_protected() {
    let _ = schtasks(&["/Delete", "/TN", TASK_NAME, "/F"]);
    let _ = schtasks(&["/Delete", "/TN", AUTOSTART_TASK_NAME, "/F"]);
    if let Some((dir, _)) = protected_target() {
        if dir.file_name().and_then(|name| name.to_str()) == Some("AgentsPads") {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

/// Delete first so `File::create` inherits the directory ACL. Truncating an
/// existing file would keep the previous DACL.
#[cfg(windows)]
fn install_copy(src: &std::path::Path, dest: &std::path::Path) -> std::io::Result<()> {
    if same_file(src, dest) {
        return Ok(());
    }
    match std::fs::remove_file(dest) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let mut input = std::fs::File::open(src)?;
    let mut output = std::fs::File::create(dest)?;
    std::io::copy(&mut input, &mut output)?;
    output.sync_all()
}

#[cfg(windows)]
fn same_file(a: &std::path::Path, b: &std::path::Path) -> bool {
    a == b
        || matches!(
            (std::fs::canonicalize(a), std::fs::canonicalize(b)),
            (Ok(a), Ok(b)) if a == b
        )
}

#[cfg(windows)]
fn protected_target() -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{
        FOLDERID_ProgramFilesX64, SHGetKnownFolderPath, KF_FLAG_DEFAULT,
    };
    let root = unsafe {
        let text = SHGetKnownFolderPath(&FOLDERID_ProgramFilesX64, KF_FLAG_DEFAULT, None).ok()?;
        let root = text.to_string().ok();
        CoTaskMemFree(Some(text.as_ptr().cast()));
        root?
    };
    let (dir, exe) = protected_install_paths(&root)?;
    let dir = std::path::PathBuf::from(dir);
    if !no_reparse_ancestors(&dir) {
        return None;
    }
    Some((dir, std::path::PathBuf::from(exe)))
}

#[cfg(windows)]
fn no_reparse_ancestors(path: &std::path::Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    path.ancestors()
        .all(|part| match std::fs::symlink_metadata(part) {
            Ok(meta) => meta.file_attributes() & 0x400 == 0,
            Err(err) => err.kind() == std::io::ErrorKind::NotFound,
        })
}

#[cfg(windows)]
fn windows_user() -> Option<String> {
    let domain = std::env::var("USERDOMAIN").ok()?;
    let name = std::env::var("USERNAME").ok()?;
    if domain.is_empty()
        || name.is_empty()
        || domain.chars().any(char::is_control)
        || name.chars().any(char::is_control)
    {
        return None;
    }
    Some(format!(r"{domain}\{name}"))
}

#[cfg(windows)]
fn write_utf16(path: &std::path::Path, xml: &str) -> std::io::Result<()> {
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(xml.encode_utf16().flat_map(u16::to_le_bytes));
    std::fs::write(path, bytes)
}

#[cfg(windows)]
fn wait_for_ready(process: Option<&std::os::windows::io::OwnedHandle>) -> bool {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{GetProcessId, WaitForSingleObject};
    let handle = process.map(|p| HANDLE(p.as_raw_handle()));
    let expected_pid = handle.map(|h| unsafe { GetProcessId(h) });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        if verified_ready(expected_pid).is_some() {
            return true;
        }
        if handle.is_some_and(|h| unsafe { WaitForSingleObject(h, 0) == WAIT_OBJECT_0 }) {
            return verified_ready(None).is_some();
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    process.is_some() && verified_ready(None).is_some()
}

#[cfg(windows)]
fn schtasks(args: &[&str]) -> bool {
    system32("schtasks.exe", args)
}

#[cfg(windows)]
fn system32(tool: &str, args: &[&str]) -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    std::process::Command::new(std::path::PathBuf::from(root).join("System32").join(tool))
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(windows)]
enum LaunchError {
    Cancelled,
    Failed,
}

#[cfg(windows)]
fn shell_exec(
    file: &std::ffi::OsStr,
    params: &str,
    hidden: bool,
) -> Result<std::os::windows::io::OwnedHandle, LaunchError> {
    use std::os::windows::io::FromRawHandle;
    use windows::core::{w, HSTRING};
    use windows::Win32::Foundation::{ERROR_CANCELLED, WIN32_ERROR};
    use windows::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_SHOWNORMAL};
    let file = HSTRING::from(file);
    let params = HSTRING::from(params);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        lpVerb: if is_elevated() {
            w!("open")
        } else {
            w!("runas")
        },
        lpFile: windows::core::PCWSTR(file.as_ptr()),
        lpParameters: windows::core::PCWSTR(params.as_ptr()),
        nShow: if hidden { SW_HIDE.0 } else { SW_SHOWNORMAL.0 },
        ..Default::default()
    };
    unsafe {
        if let Err(err) = ShellExecuteExW(&mut info) {
            return Err(if WIN32_ERROR::from_error(&err) == Some(ERROR_CANCELLED) {
                LaunchError::Cancelled
            } else {
                LaunchError::Failed
            });
        }
        if info.hProcess.is_invalid() {
            return Err(LaunchError::Failed);
        }
        Ok(std::os::windows::io::OwnedHandle::from_raw_handle(
            info.hProcess.0,
        ))
    }
}

#[cfg(windows)]
fn shell_exec_wait(file: &std::ffi::OsStr, params: &str) -> Result<(), &'static str> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
    let process = shell_exec(file, params, true).map_err(|err| match err {
        LaunchError::Cancelled => "已取消 UAC",
        LaunchError::Failed => "无法启动管理员安装程序",
    })?;
    let handle = HANDLE(process.as_raw_handle());
    let mut code = 1;
    unsafe {
        if WaitForSingleObject(handle, 30_000) != WAIT_OBJECT_0 {
            return Err("等待管理员安装程序超时或失败");
        }
        if GetExitCodeProcess(handle, &mut code).is_err() || code != 0 {
            return Err("管理员安装程序执行失败");
        }
    }
    Ok(())
}

#[cfg(windows)]
fn launch_protected_elevated() -> Result<std::os::windows::io::OwnedHandle, LaunchError> {
    let (_, exe) = protected_target().ok_or(LaunchError::Failed)?;
    if !exe.is_file() || !no_reparse_ancestors(&exe) {
        return Err(LaunchError::Failed);
    }
    shell_exec(exe.as_os_str(), RELAUNCH_FLAG, false)
}

#[cfg(windows)]
fn wait_for_pid(pid: u32, timeout: std::time::Duration) -> Result<(), &'static str> {
    use windows::Win32::Foundation::{
        CloseHandle, ERROR_INVALID_PARAMETER, WAIT_OBJECT_0, WAIT_TIMEOUT, WIN32_ERROR,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
    };
    unsafe {
        let handle = match OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
            Ok(handle) => handle,
            Err(err) if WIN32_ERROR::from_error(&err) == Some(ERROR_INVALID_PARAMETER) => {
                return Ok(())
            }
            Err(_) => return Err("无法确认旧进程已退出"),
        };
        let result =
            WaitForSingleObject(handle, u32::try_from(timeout.as_millis()).unwrap_or(10_000));
        let _ = CloseHandle(handle);
        match result {
            WAIT_OBJECT_0 => Ok(()),
            WAIT_TIMEOUT => Err("等待旧进程退出超时"),
            _ => Err("等待旧进程失败"),
        }
    }
}

#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct ReadyRecord {
    pid: u32,
    created: u64,
    session: u32,
    hwnd: isize,
}

#[cfg(any(windows, test))]
fn ready_matches(
    record: &ReadyRecord,
    observed: &ReadyRecord,
    expected_pid: Option<u32>,
    elevated: bool,
    listener: bool,
) -> bool {
    record == observed
        && record.pid != 0
        && record.created != 0
        && record.hwnd != 0
        && expected_pid.is_none_or(|pid| pid == record.pid)
        && elevated
        && listener
}

#[cfg(windows)]
fn process_facts(pid: u32, hwnd: isize) -> Option<(ReadyRecord, bool, std::path::PathBuf)> {
    use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, STILL_ACTIVE};
    use windows::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, GetProcessTimes, OpenProcess, OpenProcessToken,
        QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let facts = (|| {
            let mut code = 0;
            GetExitCodeProcess(process, &mut code).ok()?;
            if code != STILL_ACTIVE.0 as u32 {
                return None;
            }
            let mut created = FILETIME::default();
            let mut exit = FILETIME::default();
            let mut kernel = FILETIME::default();
            let mut user = FILETIME::default();
            GetProcessTimes(process, &mut created, &mut exit, &mut kernel, &mut user).ok()?;
            let mut session = 0;
            ProcessIdToSessionId(pid, &mut session).ok()?;
            let mut path = [0u16; 32768];
            let mut len = path.len() as u32;
            QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                windows::core::PWSTR(path.as_mut_ptr()),
                &mut len,
            )
            .ok()?;
            let mut token = HANDLE::default();
            OpenProcessToken(process, TOKEN_QUERY, &mut token).ok()?;
            let mut elevation = TOKEN_ELEVATION::default();
            let mut returned = 0;
            let ok = GetTokenInformation(
                token,
                TokenElevation,
                Some((&mut elevation as *mut TOKEN_ELEVATION).cast()),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut returned,
            )
            .is_ok();
            let _ = CloseHandle(token);
            if !ok {
                return None;
            }
            Some((
                ReadyRecord {
                    pid,
                    created: (u64::from(created.dwHighDateTime) << 32)
                        | u64::from(created.dwLowDateTime),
                    session,
                    hwnd,
                },
                elevation.TokenIsElevated != 0,
                std::path::PathBuf::from(String::from_utf16(&path[..len as usize]).ok()?),
            ))
        })();
        let _ = CloseHandle(process);
        facts
    }
}

#[cfg(windows)]
fn owns_listener(pid: u32) -> bool {
    use windows::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCPROW_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
    };
    let mut size = 0;
    unsafe {
        let _ = GetExtendedTcpTable(None, &mut size, false, 2, TCP_TABLE_OWNER_PID_LISTENER, 0);
        if !(4..=16 * 1024 * 1024).contains(&size) {
            return false;
        }
        let mut bytes = vec![0u8; size as usize];
        if GetExtendedTcpTable(
            Some(bytes.as_mut_ptr().cast()),
            &mut size,
            false,
            2,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        ) != 0
        {
            return false;
        }
        let count = u32::from_ne_bytes(bytes[..4].try_into().unwrap()) as usize;
        let row_size = std::mem::size_of::<MIB_TCPROW_OWNER_PID>();
        if count > (bytes.len() - 4) / row_size {
            return false;
        }
        (0..count).any(|i| {
            let row = std::ptr::read_unaligned(
                bytes
                    .as_ptr()
                    .add(4 + i * row_size)
                    .cast::<MIB_TCPROW_OWNER_PID>(),
            );
            row.dwOwningPid == pid
                && row.dwLocalAddr == 0
                && u16::from_be(row.dwLocalPort as u16) == crate::protocol::PORT
        })
    }
}

#[cfg(windows)]
fn ready_path() -> Option<std::path::PathBuf> {
    let path = protected_state_dir()?.join("ready.json");
    no_reparse_ancestors(&path).then_some(path)
}

#[cfg(windows)]
pub fn publish_ready(hwnd: isize) -> std::io::Result<()> {
    let failure = || std::io::Error::other("cannot verify elevated readiness");
    if !is_elevated() || !owns_listener(std::process::id()) {
        return Err(failure());
    }
    let (record, elevated, exe) = process_facts(std::process::id(), hwnd).ok_or_else(failure)?;
    let (_, protected) = protected_target().ok_or_else(failure)?;
    if !elevated || !same_file(&exe, &protected) || !window_belongs_to(hwnd, record.pid) {
        return Err(failure());
    }
    let path = ready_path().ok_or_else(failure)?;
    std::fs::create_dir_all(path.parent().ok_or_else(failure)?)?;
    std::fs::write(&path, serde_json::to_vec(&record)?)?;
    let _ = std::fs::remove_file(path.with_file_name(STARTUP_ERROR_FILE));
    Ok(())
}

#[cfg(windows)]
fn window_belongs_to(raw: isize, pid: u32) -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowThreadProcessId, IsWindow};
    let hwnd = HWND(raw as *mut std::ffi::c_void);
    let mut owner = 0;
    unsafe {
        IsWindow(Some(hwnd)).as_bool()
            && GetWindowThreadProcessId(hwnd, Some(&mut owner)) != 0
            && owner == pid
    }
}

#[cfg(windows)]
fn verified_ready(expected_pid: Option<u32>) -> Option<ReadyRecord> {
    use std::io::Read;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
    use windows::Win32::UI::WindowsAndMessaging::{
        IsIconic, IsWindowVisible, SetForegroundWindow, ShowWindowAsync, SW_RESTORE, SW_SHOW,
    };
    let mut bytes = Vec::new();
    std::fs::File::open(ready_path()?)
        .ok()?
        .take(1025)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > 1024 {
        return None;
    }
    let record: ReadyRecord = serde_json::from_slice(&bytes).ok()?;
    let (observed, elevated, exe) = process_facts(record.pid, record.hwnd)?;
    let (_, protected) = protected_target()?;
    let mut session = 0;
    unsafe {
        ProcessIdToSessionId(std::process::id(), &mut session).ok()?;
    }
    if record.session != session
        || !same_file(&exe, &protected)
        || !no_reparse_ancestors(&exe)
        || !window_belongs_to(record.hwnd, record.pid)
        || !ready_matches(
            &record,
            &observed,
            expected_pid,
            elevated,
            owns_listener(record.pid),
        )
    {
        return None;
    }
    let hwnd = HWND(record.hwnd as *mut std::ffi::c_void);
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindowAsync(hwnd, SW_RESTORE);
        }
        if !IsWindowVisible(hwnd).as_bool() {
            let _ = ShowWindowAsync(hwnd, SW_SHOW);
        }
        if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
            return None;
        }
        let _ = SetForegroundWindow(hwnd);
    }
    Some(record)
}

#[cfg(windows)]
fn recent_startup_error(attempt: std::time::SystemTime) -> Option<String> {
    use std::io::Read;
    let path = protected_state_dir()?.join(STARTUP_ERROR_FILE);
    let meta = std::fs::symlink_metadata(&path).ok()?;
    if meta.file_type().is_symlink() || !meta.is_file() || meta.len() > STARTUP_ERROR_MAX as u64 {
        return None;
    }
    let modified = meta.modified().ok()?;
    let now = std::time::SystemTime::now();
    if !startup_note_is_current(modified, now, attempt) {
        return None;
    }
    let mut buf = Vec::new();
    std::fs::File::open(&path)
        .ok()?
        .take(STARTUP_ERROR_MAX as u64 + 1)
        .read_to_end(&mut buf)
        .ok()?;
    let age = now.duration_since(modified).ok()?;
    accept_startup_error(&buf, age)
}

/// Marker plus the protected copy. A user-writable preference cannot satisfy either.
#[cfg(any(windows, test))]
fn admin_mode_enabled(marker: bool, exe_exists: bool) -> bool {
    marker && exe_exists
}

#[cfg(any(windows, test))]
fn protected_install_paths(program_files: &str) -> Option<(String, String)> {
    let root = program_files.trim_end_matches(['\\', '/']);
    if root.is_empty() || root.chars().all(char::is_whitespace) {
        return None;
    }
    let dir = format!(r"{root}\AgentsPads");
    let exe = format!(r"{dir}\agentspads.exe");
    Some((dir, exe))
}

#[cfg(any(windows, test))]
enum EarlyAction {
    None,
    Install(String, bool),
    Remove,
    Bad,
}

#[cfg(any(windows, test))]
fn early_action<I, S>(args: I) -> EarlyAction
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let args: Vec<String> = args.into_iter().map(|a| a.as_ref().to_string()).collect();
    let mut args = args.iter().map(String::as_str);
    while let Some(arg) = args.next() {
        match arg {
            REMOVE_FLAG => return EarlyAction::Remove,
            INSTALL_FLAG => {
                let Some(user) = args.next() else {
                    return EarlyAction::Bad;
                };
                if user.is_empty() || user.starts_with('-') || user.chars().any(char::is_control) {
                    return EarlyAction::Bad;
                }
                let autostart = args.any(|a| a == AUTOSTART_FLAG);
                return EarlyAction::Install(user.to_string(), autostart);
            }
            _ => {}
        }
    }
    EarlyAction::None
}

/// `CommandLineToArgvW` quoting. Backslashes are only special before a quote.
#[cfg(any(windows, test))]
fn quote_windows_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.chars().any(|c| matches!(c, ' ' | '\t' | '"')) {
        return arg.to_string();
    }
    let mut out = String::from("\"");
    let mut backslashes = 0usize;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            other => {
                out.extend(std::iter::repeat_n('\\', backslashes));
                out.push(other);
                backslashes = 0;
            }
        }
    }
    out.extend(std::iter::repeat_n('\\', backslashes * 2));
    out.push('"');
    out
}

#[cfg(any(windows, test))]
fn join_windows_args(args: &[&str]) -> String {
    args.iter()
        .copied()
        .map(quote_windows_arg)
        .collect::<Vec<_>>()
        .join(" ")
}

/// `logon: None` is the on-demand task (no trigger, used by manual launches).
/// `Some(enabled)` is the login-autostart task: same action, plus a delayed
/// logon trigger, and the whole task is switched with `schtasks /Change
/// /ENABLE|/DISABLE` (a disabled task cannot be run on demand, so it must be a
/// separate task). Priority 4 keeps the process at normal priority; the task
/// default (7) would lag input injection.
#[cfg(any(windows, test))]
fn task_xml(exe: &str, user: Option<&str>, logon: Option<bool>) -> String {
    let esc = |s: &str| {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    };
    let user = user
        .map(|u| format!("<UserId>{}</UserId>", esc(u)))
        .unwrap_or_default();
    let triggers = if logon.is_some() {
        format!(
            "<Triggers><LogonTrigger><Enabled>true</Enabled>{user}<Delay>{LOGON_DELAY}</Delay></LogonTrigger></Triggers>"
        )
    } else {
        String::new()
    };
    let enabled = if logon == Some(false) {
        "\n    <Enabled>false</Enabled>"
    } else {
        ""
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Description>{TASK_NAME}</Description></RegistrationInfo>
  {triggers}
  <Principals>
    <Principal id="Author">{user}<LogonType>InteractiveToken</LogonType><RunLevel>HighestAvailable</RunLevel></Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>4</Priority>{enabled}
  </Settings>
  <Actions Context="Author">
    <Exec><Command>{}</Command><Arguments>{RELAUNCH_FLAG}</Arguments></Exec>
  </Actions>
</Task>
"#,
        esc(exe)
    )
}

#[cfg(any(windows, test))]
fn handoff_parent_pid<I, S>(args: I) -> Option<u32>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let args: Vec<String> = args.into_iter().map(|a| a.as_ref().to_string()).collect();
    args.windows(2).find_map(|pair| {
        if pair[0] != HANDOFF_FLAG {
            return None;
        }
        let pid = pair[1].parse::<u32>().ok()?;
        (pid != 0).then_some(pid)
    })
}

#[cfg(any(windows, test))]
fn explain_handoff_failure(
    task_started: bool,
    cancelled: bool,
    startup_error: Option<&str>,
) -> String {
    if cancelled {
        return "已取消 UAC".to_string();
    }
    if let Some(detail) = startup_error.map(str::trim).filter(|text| !text.is_empty()) {
        return format!("管理员实例未就绪：{detail}");
    }
    if task_started {
        "等待管理员实例就绪超时".to_string()
    } else {
        "计划任务及 UAC 回退未能启动管理员实例".to_string()
    }
}

/// Drop path-like or huge details. The note is readable by ordinary users.
#[cfg(any(windows, test))]
fn startup_failure_text(kind: &str, detail: &str) -> String {
    let detail = detail.split(['\r', '\n']).next().unwrap_or("").trim();
    let usable = !detail.is_empty()
        && detail != kind
        && detail.chars().count() <= 80
        && !detail
            .chars()
            .any(|c| c == '\\' || c == '/' || c.is_control());
    if usable {
        format!("{kind}: {detail}")
    } else {
        kind.to_string()
    }
}

#[cfg(any(windows, test))]
fn startup_error_line(reason: &str, unix_secs: u64) -> String {
    let reason = reason.split(['\r', '\n']).next().unwrap_or("").trim();
    let reason = if reason.is_empty() {
        "startup failed"
    } else {
        reason
    };
    let stamp = unix_utc_stamp(unix_secs);
    format!("{reason} {stamp}\n")
}

#[cfg(any(windows, test))]
fn accept_startup_error(bytes: &[u8], age: std::time::Duration) -> Option<String> {
    if bytes.is_empty() || bytes.len() > STARTUP_ERROR_MAX || age > STARTUP_ERROR_MAX_AGE {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let line = text.lines().next()?.trim();
    if line.is_empty() || line.chars().any(char::is_control) {
        return None;
    }
    Some(line.to_string())
}

#[cfg(any(windows, test))]
fn startup_note_is_current(
    modified: std::time::SystemTime,
    now: std::time::SystemTime,
    attempt: std::time::SystemTime,
) -> bool {
    if modified < attempt {
        return false;
    }
    now.duration_since(modified)
        .is_ok_and(|age| age <= STARTUP_ERROR_MAX_AGE)
}

#[cfg(any(windows, test))]
fn unix_utc_stamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let tod = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    let h = tod / 3_600;
    let min = (tod % 3_600) / 60;
    let s = tod % 60;
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{min:02}:{s:02}Z")
}

/// Days since 1970-01-01 to a civil date. Howard Hinnant, public domain.
#[cfg(any(windows, test))]
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elevation_attempt_cannot_accept_an_ordinary_token() {
        assert!(reject_ordinary_attempt(true, false));
        assert!(!reject_ordinary_attempt(true, true));
        assert!(!reject_ordinary_attempt(false, false));
    }

    #[test]
    fn ready_requires_the_same_live_elevated_listener_and_window() {
        let ready = ReadyRecord {
            pid: 42,
            created: 100,
            session: 1,
            hwnd: 88,
        };
        assert!(ready_matches(&ready, &ready, Some(42), true, true));
        assert!(!ready_matches(&ready, &ready, Some(43), true, true));
        assert!(!ready_matches(&ready, &ready, None, false, true));
        assert!(!ready_matches(&ready, &ready, None, true, false));
        for observed in [
            ReadyRecord { pid: 43, ..ready },
            ReadyRecord {
                created: 101,
                ..ready
            },
            ReadyRecord {
                session: 2,
                ..ready
            },
            ReadyRecord { hwnd: 0, ..ready },
        ] {
            assert!(!ready_matches(&ready, &observed, None, true, true));
        }
    }

    #[test]
    fn task_runs_elevated_on_demand_at_normal_priority() {
        let (_dir, exe) = protected_install_paths(r"C:\Program Files").unwrap();
        let xml = task_xml(&exe, Some(r"PC\jo"), None);
        assert!(xml.contains("<RunLevel>HighestAvailable</RunLevel>"));
        assert!(xml.contains("<LogonType>InteractiveToken</LogonType>"));
        assert!(xml.contains(r"<UserId>PC\jo</UserId>"));
        assert!(xml.contains(&format!("<Command>{exe}</Command>")));
        assert!(xml.contains(r"<Command>C:\Program Files\AgentsPads\agentspads.exe</Command>"));
        assert!(xml.contains("<Arguments>--elevated</Arguments>"));
        assert!(xml.contains("<Priority>4</Priority>"));
        assert!(xml.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
        assert!(!xml.contains("Trigger"));
        assert!(!task_xml("a.exe", None, None).contains("UserId"));
        let escaped = task_xml(r"C:\Apps\A&B <x>\agentpad.exe", Some(r"PC\jo"), None);
        assert!(escaped.contains(r"<Command>C:\Apps\A&amp;B &lt;x&gt;\agentpad.exe</Command>"));
    }

    #[test]
    fn autostart_task_has_delayed_logon_trigger_and_switchable_state() {
        let on = task_xml("a.exe", Some(r"PC\jo"), Some(true));
        assert!(on.contains("<LogonTrigger>"));
        assert!(on.contains(r"<UserId>PC\jo</UserId><Delay>PT30S</Delay>"));
        assert!(on.contains("<RunLevel>HighestAvailable</RunLevel>"));
        assert!(on.contains("<Arguments>--elevated</Arguments>"));
        assert!(!on.contains("<Enabled>false</Enabled>"));
        let off = task_xml("a.exe", Some(r"PC\jo"), Some(false));
        assert!(off.contains("<LogonTrigger>"));
        assert!(off.contains("<Enabled>false</Enabled>"));
        assert_ne!(AUTOSTART_TASK_NAME, TASK_NAME);
    }

    #[test]
    fn admin_secret_dir_is_locked_to_admins_and_system() {
        let (dir, _) = protected_install_paths(r"C:\Program Files").unwrap();
        assert_eq!(dir, r"C:\Program Files\AgentsPads");
        let [owner, acl] = lock_commands(r"C:\Program Files\AgentsPads\secret");
        assert_eq!(owner[1..3], ["/setowner", "*S-1-5-32-544"]);
        assert!(acl.contains(&"/inheritance:r".to_string()));
        let grants: Vec<_> = acl.iter().filter(|a| a.starts_with('*')).collect();
        assert_eq!(grants.len(), 2);
        assert!(grants
            .iter()
            .all(|g| g.starts_with("*S-1-5-32-544") || g.starts_with("*S-1-5-18")));
        assert_eq!(SECRET_DIR, "secret");
    }

    #[test]
    fn protected_install_paths_reject_empty_roots() {
        assert_eq!(
            protected_install_paths(r"C:\Program Files\"),
            Some((
                r"C:\Program Files\AgentsPads".to_string(),
                r"C:\Program Files\AgentsPads\agentspads.exe".to_string(),
            ))
        );
        assert_eq!(
            protected_install_paths(r"D:\PF\\"),
            Some((
                r"D:\PF\AgentsPads".to_string(),
                r"D:\PF\AgentsPads\agentspads.exe".to_string(),
            ))
        );
        assert_eq!(protected_install_paths(""), None);
        assert_eq!(protected_install_paths(r"\\"), None);
        assert_eq!(protected_install_paths("   "), None);
    }

    #[test]
    fn admin_mode_requires_both_marker_and_protected_copy() {
        assert!(admin_mode_enabled(true, true));
        assert!(!admin_mode_enabled(true, false));
        assert!(!admin_mode_enabled(false, true));
    }

    #[test]
    fn admin_maintenance_args_are_quoted_and_parsed() {
        assert_eq!(
            join_windows_args(&[INSTALL_FLAG, r"PC\jo"]),
            r"--install-admin-task PC\jo"
        );
        assert_eq!(
            join_windows_args(&[INSTALL_FLAG, r"PC\Jo Ann"]),
            r#"--install-admin-task "PC\Jo Ann""#
        );
        assert_eq!(quote_windows_arg("a\"b"), r#""a\"b""#);
        assert_eq!(quote_windows_arg("a b\\"), r#""a b\\""#);
        assert!(matches!(
            early_action(["agentspads.exe", INSTALL_FLAG, r"PC\jo"]),
            EarlyAction::Install(user, false) if user == r"PC\jo"
        ));
        assert!(matches!(
            early_action(["agentspads.exe", INSTALL_FLAG, r"PC\jo", AUTOSTART_FLAG]),
            EarlyAction::Install(user, true) if user == r"PC\jo"
        ));
        assert!(matches!(
            early_action(["agentspads.exe", REMOVE_FLAG]),
            EarlyAction::Remove
        ));
        assert!(matches!(
            early_action(["agentspads.exe", "--elevated"]),
            EarlyAction::None
        ));
        assert!(matches!(
            early_action(["agentspads.exe", INSTALL_FLAG]),
            EarlyAction::Bad
        ));
        assert!(matches!(
            early_action(["agentspads.exe", INSTALL_FLAG, "--elevated"]),
            EarlyAction::Bad
        ));
    }

    #[test]
    fn handoff_pid_is_parsed_without_update_semantics() {
        assert_eq!(
            handoff_parent_pid(["agentspads.exe", "--handoff", "42"]),
            Some(42)
        );
        assert_eq!(handoff_parent_pid(["agentspads.exe", "--handoff"]), None);
        assert_eq!(
            handoff_parent_pid(["agentspads.exe", "--handoff", "0"]),
            None
        );
        assert_eq!(
            handoff_parent_pid(["agentspads.exe", "--handoff", "nope"]),
            None
        );
        assert_eq!(
            handoff_parent_pid(["agentspads.exe", "--after-update", "7"]),
            None
        );
        assert_eq!(handoff_parent_pid(["agentspads.exe", "--elevated"]), None);
        assert!(matches!(
            early_action(["agentspads.exe", "--handoff", "9"]),
            EarlyAction::None
        ));
    }

    #[test]
    fn handoff_failure_reasons_stay_distinct() {
        assert_eq!(explain_handoff_failure(false, true, None), "已取消 UAC");
        assert_eq!(
            explain_handoff_failure(true, true, Some("identity store failed")),
            "已取消 UAC"
        );
        assert_eq!(
            explain_handoff_failure(true, false, Some("identity store failed: icacls failed")),
            "管理员实例未就绪：identity store failed: icacls failed"
        );
        assert_eq!(
            explain_handoff_failure(false, false, None),
            "计划任务及 UAC 回退未能启动管理员实例"
        );
        assert_eq!(
            explain_handoff_failure(true, false, None),
            "等待管理员实例就绪超时"
        );
        assert_eq!(
            explain_handoff_failure(false, false, Some("listen failed")),
            "管理员实例未就绪：listen failed"
        );
        assert_eq!(
            explain_handoff_failure(true, false, Some("  ")),
            "等待管理员实例就绪超时"
        );
    }

    #[test]
    fn startup_error_note_is_short_fresh_and_under_program_files() {
        use std::time::{Duration, SystemTime};
        assert_eq!(unix_utc_stamp(0), "1970-01-01T00:00:00Z");
        assert_eq!(unix_utc_stamp(946_684_800), "2000-01-01T00:00:00Z");
        assert_eq!(unix_utc_stamp(1_709_210_096), "2024-02-29T12:34:56Z");
        assert_eq!(unix_utc_stamp(1_791_371_580), "2026-10-07T11:13:00Z");
        assert_eq!(
            startup_failure_text("identity store failed", "icacls failed"),
            "identity store failed: icacls failed"
        );
        assert_eq!(
            startup_failure_text("identity store failed", r"C:\Users\secret"),
            "identity store failed"
        );
        assert_eq!(
            startup_failure_text("identity store failed", "identity store failed"),
            "identity store failed"
        );
        assert_eq!(startup_failure_text("listen failed", ""), "listen failed");
        assert_eq!(
            startup_failure_text("identity store failed", &"x".repeat(81)),
            "identity store failed"
        );
        let line = startup_error_line("identity store failed: icacls failed", 946_684_800);
        assert_eq!(
            line,
            "identity store failed: icacls failed 2000-01-01T00:00:00Z\n"
        );
        assert_eq!(
            accept_startup_error(line.as_bytes(), Duration::from_secs(1)).as_deref(),
            Some("identity store failed: icacls failed 2000-01-01T00:00:00Z")
        );
        assert!(accept_startup_error(line.as_bytes(), STARTUP_ERROR_MAX_AGE).is_some());
        assert_eq!(
            accept_startup_error(
                line.as_bytes(),
                STARTUP_ERROR_MAX_AGE + Duration::from_secs(1)
            ),
            None
        );
        assert_eq!(accept_startup_error(b"", Duration::ZERO), None);
        assert_eq!(accept_startup_error(b"\n\n", Duration::ZERO), None);
        assert_eq!(accept_startup_error(&[0xff, 0xfe], Duration::ZERO), None);
        assert_eq!(
            accept_startup_error(b"ok\nignored", Duration::ZERO).as_deref(),
            Some("ok")
        );
        assert_eq!(accept_startup_error(b"bad\tline", Duration::ZERO), None);
        assert_eq!(
            accept_startup_error(&vec![b'a'; STARTUP_ERROR_MAX + 1], Duration::ZERO),
            None
        );
        assert_eq!(
            accept_startup_error(&vec![b'a'; STARTUP_ERROR_MAX], Duration::ZERO)
                .as_deref()
                .map(str::len),
            Some(STARTUP_ERROR_MAX)
        );
        let t = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        assert!(startup_note_is_current(t, t, t));
        assert!(!startup_note_is_current(t - Duration::from_secs(1), t, t));
        assert!(startup_note_is_current(t, t + STARTUP_ERROR_MAX_AGE, t));
        assert!(!startup_note_is_current(
            t,
            t + STARTUP_ERROR_MAX_AGE + Duration::from_secs(1),
            t
        ));
        assert!(!startup_note_is_current(t + Duration::from_secs(5), t, t));
        assert_eq!(STARTUP_ERROR_FILE, "startup_error.txt");
    }
}
