//! Windows only: UIPI blocks `SendInput` into windows of higher integrity
//! (Task Manager, elevated apps), so users can opt into running elevated.
//! One UAC prompt copies this exe to `%ProgramFiles%\AgentsPads\agentspads.exe`
//! (ACL inherited from Program Files) and registers an on-demand "highest
//! privileges" task that runs that copy. Later launches, including login
//! autostart, re-exec through the task with no prompt. Task XML is written
//! only in that directory and removed after `schtasks /Create`.

#[cfg(any(windows, test))]
const TASK_NAME: &str = "AgentsPads Elevated";
#[cfg(windows)]
const PREFERENCE_FILE: &str = "run_as_admin.txt";
#[cfg(windows)]
const LAUNCHER_FILE: &str = "admin_launcher.txt";
#[cfg(windows)]
const TASK_XML_NAME: &str = "agentspads-task.xml";
#[cfg(any(windows, test))]
const SECRET_DIR: &str = "secret";
const RELAUNCH_FLAG: &str = "--elevated";
#[cfg(any(windows, test))]
const INSTALL_FLAG: &str = "--install-admin-task";
#[cfg(any(windows, test))]
const REMOVE_FLAG: &str = "--remove-admin-task";

/// The elevated child may start before the parent has released the port.
pub fn relaunched() -> bool {
    std::env::args().any(|a| a == RELAUNCH_FLAG)
}

/// Call before binding the port; true means this process should exit.
pub fn relaunch_if_needed() -> bool {
    #[cfg(windows)]
    {
        if !enabled() || is_elevated() || relaunched() {
            return false;
        }
        if run_task() && wait_for_listener() {
            return true;
        }
        // Task missing or the protected copy is gone: run unelevated and show
        // the switch as off so the user can re-register it.
        crate::logutil::write("admin relaunch failed");
        let _ = set_enabled(false);
        false
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// `--install-admin-task` / `--remove-admin-task`: do the work and exit.
pub fn exit_if_admin_maintenance() {
    #[cfg(windows)]
    {
        let code = match early_action(std::env::args()) {
            EarlyAction::None => return,
            EarlyAction::Install(user) => {
                if install_protected(&user) {
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

#[cfg(windows)]
pub fn enabled() -> bool {
    std::fs::read_to_string(crate::identity::data_dir().join(PREFERENCE_FILE))
        .is_ok_and(|v| v.trim() == "true")
}

#[cfg(windows)]
pub fn set_enabled(on: bool) -> std::io::Result<()> {
    let dir = crate::identity::data_dir();
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join(PREFERENCE_FILE), on.to_string())
}

#[cfg(windows)]
pub fn is_elevated() -> bool {
    unsafe { windows::Win32::UI::Shell::IsUserAnAdmin().as_bool() }
}

/// One UAC prompt, unless this process is already elevated.
#[cfg(windows)]
pub fn install_task() -> bool {
    let (Ok(exe), Some(user)) = (std::env::current_exe(), windows_user()) else {
        return false;
    };
    if protected_target().is_none_or(|(_, dest)| !same_file(&exe, &dest)) {
        remember_launcher(&exe);
    }
    shell_exec_wait(
        exe.as_os_str(),
        &join_windows_args(&[INSTALL_FLAG, user.as_str()]),
    )
}

#[cfg(windows)]
pub(crate) fn launcher_for_autostart(current: std::path::PathBuf) -> std::path::PathBuf {
    let Some((_, protected)) = protected_target() else {
        return current;
    };
    if !same_file(&current, &protected) {
        return current;
    }
    let Ok(text) = std::fs::read_to_string(crate::identity::data_dir().join(LAUNCHER_FILE)) else {
        return current;
    };
    let path = std::path::PathBuf::from(text.trim());
    if path.is_file() {
        path
    } else {
        current
    }
}

#[cfg(windows)]
fn remember_launcher(path: &std::path::Path) {
    let Some(text) = path.to_str() else {
        return;
    };
    let dir = crate::identity::data_dir();
    if std::fs::create_dir_all(&dir).is_ok() {
        let _ = std::fs::write(dir.join(LAUNCHER_FILE), text);
    }
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
fn install_protected(user: &str) -> bool {
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
    if write_utf16(&xml_path, &task_xml(command, Some(user))).is_err() {
        return false;
    }
    let ok = xml_path
        .to_str()
        .is_some_and(|xml| schtasks(&["/Create", "/TN", TASK_NAME, "/XML", xml, "/F"]));
    let _ = std::fs::remove_file(xml_path);
    ok
}

#[cfg(windows)]
fn remove_protected() {
    let _ = schtasks(&["/Delete", "/TN", TASK_NAME, "/F"]);
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
    let w6432 = std::env::var("ProgramW6432").ok();
    let files = std::env::var("ProgramFiles").ok();
    let root = program_files_root(w6432.as_deref(), files.as_deref())?;
    let (dir, exe) = protected_install_paths(root)?;
    Some((std::path::PathBuf::from(dir), std::path::PathBuf::from(exe)))
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
fn wait_for_listener() -> bool {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], crate::protocol::PORT));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    while std::time::Instant::now() < deadline {
        if std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(300))
            .is_ok()
        {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    false
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

/// Runs `file` elevated (or directly, if this process already is) and waits.
#[cfg(windows)]
fn shell_exec_wait(file: &std::ffi::OsStr, params: &str) -> bool {
    use windows::core::{w, HSTRING};
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let file = HSTRING::from(file);
    let params = HSTRING::from(params);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: if is_elevated() {
            w!("open")
        } else {
            w!("runas")
        },
        lpFile: windows::core::PCWSTR(file.as_ptr()),
        lpParameters: windows::core::PCWSTR(params.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    unsafe {
        if ShellExecuteExW(&mut info).is_err() || info.hProcess.is_invalid() {
            return false;
        }
        WaitForSingleObject(info.hProcess, INFINITE);
        let mut code = 1u32;
        let got = GetExitCodeProcess(info.hProcess, &mut code).is_ok();
        let _ = CloseHandle(info.hProcess);
        got && code == 0
    }
}

#[cfg(any(windows, test))]
fn program_files_root<'a>(
    program_w6432: Option<&'a str>,
    program_files: Option<&'a str>,
) -> Option<&'a str> {
    program_w6432
        .filter(|text| !text.is_empty())
        .or_else(|| program_files.filter(|text| !text.is_empty()))
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
    Install(String),
    Remove,
    Bad,
}

#[cfg(any(windows, test))]
fn early_action<I, S>(args: I) -> EarlyAction
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_ref() {
            REMOVE_FLAG => return EarlyAction::Remove,
            INSTALL_FLAG => {
                let Some(user) = args.next() else {
                    return EarlyAction::Bad;
                };
                let user = user.as_ref();
                if user.is_empty() || user.starts_with('-') || user.chars().any(char::is_control) {
                    return EarlyAction::Bad;
                }
                return EarlyAction::Install(user.to_string());
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

/// On-demand only (no trigger): login autostart still goes through the normal
/// startup entry, which then re-execs via this task. Priority 4 keeps the
/// process at normal priority; the task default (7) would lag input injection.
#[cfg(any(windows, test))]
fn task_xml(exe: &str, user: Option<&str>) -> String {
    let esc = |s: &str| {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    };
    let user = user
        .map(|u| format!("<UserId>{}</UserId>", esc(u)))
        .unwrap_or_default();
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Description>{TASK_NAME}</Description></RegistrationInfo>
  <Principals>
    <Principal id="Author">{user}<LogonType>InteractiveToken</LogonType><RunLevel>HighestAvailable</RunLevel></Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>4</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec><Command>{}</Command><Arguments>{RELAUNCH_FLAG}</Arguments></Exec>
  </Actions>
</Task>
"#,
        esc(exe)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_runs_elevated_on_demand_at_normal_priority() {
        let (_dir, exe) = protected_install_paths(r"C:\Program Files").unwrap();
        let xml = task_xml(&exe, Some(r"PC\jo"));
        assert!(xml.contains("<RunLevel>HighestAvailable</RunLevel>"));
        assert!(xml.contains("<LogonType>InteractiveToken</LogonType>"));
        assert!(xml.contains(r"<UserId>PC\jo</UserId>"));
        assert!(xml.contains(&format!("<Command>{exe}</Command>")));
        assert!(xml.contains(r"<Command>C:\Program Files\AgentsPads\agentspads.exe</Command>"));
        assert!(xml.contains("<Arguments>--elevated</Arguments>"));
        assert!(xml.contains("<Priority>4</Priority>"));
        assert!(xml.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
        assert!(!xml.contains("Trigger"));
        assert!(!task_xml("a.exe", None).contains("UserId"));
        let escaped = task_xml(r"C:\Apps\A&B <x>\agentpad.exe", Some(r"PC\jo"));
        assert!(escaped.contains(r"<Command>C:\Apps\A&amp;B &lt;x&gt;\agentpad.exe</Command>"));
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
    fn protected_dir_prefers_program_w6432() {
        assert_eq!(
            program_files_root(Some(r"C:\Program Files"), Some(r"C:\Program Files (x86)")),
            Some(r"C:\Program Files")
        );
        assert_eq!(program_files_root(Some(""), Some(r"D:\PF")), Some(r"D:\PF"));
        assert_eq!(program_files_root(None, None), None);
        assert_eq!(program_files_root(Some(""), Some("")), None);
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
            EarlyAction::Install(user) if user == r"PC\jo"
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
}
