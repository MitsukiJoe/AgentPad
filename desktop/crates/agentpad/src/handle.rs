use std::time::Duration;

use crate::protocol::{InMsg, OutMsg};

#[derive(Default)]
pub struct Conn {
    pub shadow: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Text(String),
    Enter,
    Undo,
    Key {
        key: String,
        modifiers: Vec<String>,
    },
    Pointer {
        dx: f64,
        dy: f64,
        buttons: u8,
        wheel: i32,
    },
}

pub fn handle(
    paused: bool,
    sync_enabled: bool,
    conn: &mut Conn,
    msg: InMsg,
) -> (Vec<OutMsg>, Vec<Action>) {
    match msg {
        InMsg::Hello { .. } => (vec![], vec![]),
        InMsg::Ping => (vec![OutMsg::Pong { sync_enabled }], vec![]),
        InMsg::Text {
            content,
            auto_enter,
            send_mode,
        } => {
            if paused {
                return (
                    vec![OutMsg::Ack {
                        ok: false,
                        clear_input: false,
                    }],
                    vec![],
                );
            }
            let actions = apply_text(conn, &content, auto_enter, &send_mode);
            (
                vec![OutMsg::Ack {
                    ok: true,
                    clear_input: send_mode != "shadow",
                }],
                actions,
            )
        }
        InMsg::Key { key, modifiers } => {
            if paused {
                return (
                    vec![OutMsg::Ack {
                        ok: false,
                        clear_input: false,
                    }],
                    vec![],
                );
            }
            (
                vec![OutMsg::Ack {
                    ok: true,
                    clear_input: false,
                }],
                vec![Action::Key { key, modifiers }],
            )
        }
        InMsg::Pointer {
            dx,
            dy,
            buttons,
            wheel,
        } => {
            if paused {
                return (
                    vec![OutMsg::Ack {
                        ok: false,
                        clear_input: false,
                    }],
                    vec![],
                );
            }
            (
                vec![],
                vec![Action::Pointer {
                    dx,
                    dy,
                    buttons,
                    wheel,
                }],
            )
        }
        InMsg::Undo => {
            if paused {
                return (
                    vec![OutMsg::Ack {
                        ok: false,
                        clear_input: false,
                    }],
                    vec![],
                );
            }
            conn.shadow.clear();
            (
                vec![OutMsg::Ack {
                    ok: true,
                    clear_input: false,
                }],
                vec![Action::Undo],
            )
        }
    }
}

fn apply_text(conn: &mut Conn, content: &str, auto_enter: bool, send_mode: &str) -> Vec<Action> {
    match send_mode {
        "shadow" => {
            if let Some(delta) = content.strip_prefix(conn.shadow.as_str()) {
                conn.shadow = content.to_string();
                if delta.is_empty() {
                    vec![]
                } else {
                    vec![Action::Text(delta.to_string())]
                }
            } else {
                vec![]
            }
        }
        "commit" | "submit" => {
            let suffix = content
                .strip_prefix(conn.shadow.as_str())
                .unwrap_or(content);
            conn.shadow.clear();
            let mut out = Vec::new();
            if !suffix.is_empty() {
                out.push(Action::Text(suffix.to_string()));
            }
            if auto_enter {
                out.push(Action::Enter);
            }
            out
        }
        _ => vec![],
    }
}

pub fn apply_actions(actions: &[Action]) {
    if !actions.is_empty() && !agentpad_input::accessibility_trusted() {
        crate::logutil::write("inject blocked: accessibility permission missing");
        return;
    }
    let mut pasted = false;
    for a in actions {
        if pasted && matches!(a, Action::Enter) {
            std::thread::sleep(Duration::from_millis(80));
        }
        let r = match a {
            Action::Text(t) => agentpad_input::inject_text(t),
            Action::Enter => agentpad_input::inject_key("Enter", &[]),
            Action::Undo => agentpad_input::undo(),
            Action::Key { key, modifiers } => {
                let mods: Vec<&str> = modifiers.iter().map(|s| s.as_str()).collect();
                agentpad_input::inject_key(key, &mods)
            }
            Action::Pointer {
                dx,
                dy,
                buttons,
                wheel,
            } => agentpad_input::inject_pointer(*dx, *dy, *buttons, *wheel),
        };
        let succeeded = r.is_ok();
        if let Err(e) = r {
            crate::logutil::write(&format!("inject: {e}"));
        } else if !matches!(a, Action::Pointer { .. }) {
            crate::logutil::write("inject ok");
        }
        pasted = succeeded && matches!(a, Action::Text(_));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shadow_appends_only() {
        let mut c = Conn::default();
        let (_, a) = handle(
            false,
            true,
            &mut c,
            InMsg::Text {
                content: "hel".into(),
                auto_enter: false,
                send_mode: "shadow".into(),
            },
        );
        assert_eq!(a, vec![Action::Text("hel".into())]);
        let (_, a) = handle(
            false,
            true,
            &mut c,
            InMsg::Text {
                content: "hello".into(),
                auto_enter: false,
                send_mode: "shadow".into(),
            },
        );
        assert_eq!(a, vec![Action::Text("lo".into())]);
    }

    #[test]
    fn shadow_mismatch_does_not_clear() {
        let mut c = Conn {
            shadow: "abc".into(),
        };
        let (_, a) = handle(
            false,
            true,
            &mut c,
            InMsg::Text {
                content: "xyz".into(),
                auto_enter: false,
                send_mode: "shadow".into(),
            },
        );
        assert!(a.is_empty());
        assert_eq!(c.shadow, "abc");
    }

    #[test]
    fn submit_pastes_suffix_then_enter() {
        let mut c = Conn {
            shadow: "hi".into(),
        };
        let (replies, a) = handle(
            false,
            true,
            &mut c,
            InMsg::Text {
                content: "hi there".into(),
                auto_enter: true,
                send_mode: "submit".into(),
            },
        );
        assert_eq!(a, vec![Action::Text(" there".into()), Action::Enter]);
        assert!(matches!(
            replies[0],
            OutMsg::Ack {
                ok: true,
                clear_input: true
            }
        ));
        assert!(c.shadow.is_empty());
    }

    #[test]
    fn pause_drops_input() {
        let mut c = Conn::default();
        let (replies, a) = handle(
            true,
            false,
            &mut c,
            InMsg::Text {
                content: "nope".into(),
                auto_enter: false,
                send_mode: "submit".into(),
            },
        );
        assert!(a.is_empty());
        assert_eq!(
            replies,
            vec![OutMsg::Ack {
                ok: false,
                clear_input: false
            }]
        );
        let (replies, a) = handle(true, false, &mut c, InMsg::Undo);
        assert!(a.is_empty());
        assert!(matches!(replies[0], OutMsg::Ack { ok: false, .. }));
        let (replies, _) = handle(true, false, &mut c, InMsg::Ping);
        assert_eq!(
            replies,
            vec![OutMsg::Pong {
                sync_enabled: false
            }]
        );
    }
}
