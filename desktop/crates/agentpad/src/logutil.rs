use std::io::Write;
use std::path::Path;
use std::sync::{Mutex, Once, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::identity;

const LIMIT: u64 = 2 * 1024 * 1024;
#[derive(Default)]
struct Session {
    enabled: bool,
    pointers: [u64; 4],
    window: Option<Instant>,
}

fn session() -> &'static Mutex<Session> {
    static SESSION: OnceLock<Mutex<Session>> = OnceLock::new();
    SESSION.get_or_init(|| Mutex::new(Session::default()))
}

pub fn enabled() -> bool {
    session().lock().unwrap().enabled
}

fn clear_files(dir: &Path) -> std::io::Result<()> {
    for name in ["agentpad.log", "agentpad.log.1"] {
        match std::fs::remove_file(dir.join(name)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

pub fn set_enabled(enabled: bool) {
    let mut state = session().lock().unwrap();
    if state.enabled == enabled {
        return;
    }
    *state = Session::default();
    if enabled && clear_files(&identity::log_dir()).is_ok() {
        state.enabled = true;
        state.window = Some(Instant::now());
        append(&identity::log_dir(), "diagnostics session started");
        static FLUSHER: Once = Once::new();
        FLUSHER.call_once(|| {
            std::thread::spawn(|| loop {
                std::thread::sleep(Duration::from_secs(1));
                flush_pointer_window();
            });
        });
    }
}

pub fn clear() {
    let mut state = session().lock().unwrap();
    state.pointers = [0; 4];
    state.window = Some(Instant::now());
    if clear_files(&identity::log_dir()).is_err() {
        state.enabled = false;
    }
}

fn append(dir: &Path, msg: &str) {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let line = format!("{ts} {msg}\n");
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let path = dir.join("agentpad.log");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() + line.len() as u64 > LIMIT) {
        let backup = dir.join("agentpad.log.1");
        let _ = std::fs::remove_file(&backup);
        if std::fs::rename(&path, backup).is_err() {
            return;
        }
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = file.write_all(line.as_bytes());
    }
}

pub fn write(msg: &'static str) {
    let state = session().lock().unwrap();
    if state.enabled {
        append(&identity::log_dir(), msg);
    }
}

pub fn operation(category: &'static str, injecting: bool, success: bool) {
    let mut state = session().lock().unwrap();
    if !state.enabled {
        return;
    }
    if category == "[鼠标位移][鼠标按键][滚轮]" {
        state.pointers[usize::from(injecting) * 2 + usize::from(!success)] += 1;
    } else {
        append(
            &identity::log_dir(),
            &format!(
                "{category} {} {}",
                if injecting { "inject" } else { "receive" },
                if success { "ok" } else { "blocked_or_failed" }
            ),
        );
    }
}

fn flush_pointer_window() {
    let mut state = session().lock().unwrap();
    if !state.enabled {
        return;
    }
    let elapsed = state.window.get_or_insert_with(Instant::now).elapsed();
    if elapsed.as_secs() < 1 {
        return;
    }
    for (index, count) in state.pointers.iter().enumerate() {
        if *count > 0 {
            append(
                &identity::log_dir(),
                &format!(
                    "[鼠标位移][鼠标按键][滚轮] {} {} count={count} window_ms={}",
                    if index < 2 { "receive" } else { "inject" },
                    if index % 2 == 0 {
                        "ok"
                    } else {
                        "blocked_or_failed"
                    },
                    elapsed.as_millis()
                ),
            );
        }
    }
    state.pointers = [0; 4];
    state.window = Some(Instant::now());
}

pub fn action_category(action: &crate::handle::Action) -> &'static str {
    use crate::handle::Action;
    match action {
        Action::Text(_) => "[文字]",
        Action::Key { .. } | Action::Enter => "[快捷键]",
        Action::Undo => "undo",
        Action::Pointer { .. } => "[鼠标位移][鼠标按键][滚轮]",
    }
}

pub fn input_category(msg: &crate::protocol::InMsg) -> &'static str {
    use crate::protocol::InMsg;
    match msg {
        InMsg::Text { .. } => "[文字]",
        InMsg::Key { .. } => "[快捷键]",
        InMsg::Pointer { .. } => "[鼠标位移][鼠标按键][滚轮]",
        InMsg::Undo => "undo",
        InMsg::Hello { .. } => "hello",
        InMsg::Pair { .. } => "pair",
        InMsg::Ping => "ping",
    }
}

pub fn open_dir() {
    let dir = identity::log_dir();
    let _ = std::fs::create_dir_all(&dir);
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(&dir).spawn();
    }
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer").arg(&dir).spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::InMsg;

    #[test]
    fn payloads_are_replaced_and_files_are_bounded_and_clearable() {
        assert!(!Session::default().enabled);
        let dir = std::env::temp_dir().join(format!(
            "agentpad-diagnostics-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let messages = [
            InMsg::Text {
                content: "SECRET\n192.168.1.42 /private/path host-uuid".into(),
                auto_enter: false,
                send_mode: "SECRET".into(),
            },
            InMsg::Key {
                key: "SECRET\nforged event".into(),
                modifiers: vec!["SECRET".into()],
            },
            InMsg::Pointer {
                dx: 12345.678,
                dy: -87654.321,
                buttons: 255,
                wheel: 123456,
            },
        ];
        for msg in &messages {
            append(&dir, input_category(msg));
        }
        let content = std::fs::read_to_string(dir.join("agentpad.log")).unwrap();
        let categories: Vec<_> = content
            .lines()
            .map(|line| line.split_once(' ').unwrap().1)
            .collect();
        assert_eq!(
            categories,
            ["[文字]", "[快捷键]", "[鼠标位移][鼠标按键][滚轮]"]
        );
        for _ in 0..3 {
            std::fs::OpenOptions::new()
                .write(true)
                .open(dir.join("agentpad.log"))
                .unwrap()
                .set_len(LIMIT)
                .unwrap();
            append(&dir, "diagnostics rotation ok");
            assert!(std::fs::metadata(dir.join("agentpad.log")).unwrap().len() <= LIMIT);
            assert_eq!(
                std::fs::metadata(dir.join("agentpad.log.1")).unwrap().len(),
                LIMIT
            );
            assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
        }
        clear_files(&dir).unwrap();
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        std::fs::remove_dir(dir).unwrap();
    }
}
