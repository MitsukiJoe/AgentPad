use serde::{Deserialize, Serialize};

pub const PORT: u16 = 9618;
pub const QR_TYPE: &str = "agentpad";
pub const QR_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct QrPayload {
    pub v: u32,
    #[serde(rename = "type")]
    pub kind: String,
    pub device_id: String,
    pub ip: String,
    pub port: u16,
    pub name: String,
    pub os: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ips: Vec<String>,
}

impl QrPayload {
    pub fn new(device_id: String, name: String, os: String, ip: String, ips: Vec<String>) -> Self {
        Self {
            v: QR_VERSION,
            kind: QR_TYPE.to_string(),
            device_id,
            ip,
            port: PORT,
            name,
            os,
            ips,
        }
    }
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum InMsg {
    #[serde(rename = "hello")]
    Hello {
        client_id: String,
        client_name: String,
    },
    #[serde(rename = "text")]
    Text {
        content: String,
        auto_enter: bool,
        send_mode: String,
    },
    #[serde(rename = "key")]
    Key {
        key: String,
        #[serde(default)]
        modifiers: Vec<String>,
    },
    #[serde(rename = "pointer")]
    Pointer {
        dx: f64,
        dy: f64,
        #[serde(default)]
        buttons: u8,
        #[serde(default)]
        wheel: i32,
    },
    #[serde(rename = "undo")]
    Undo,
    #[serde(rename = "ping")]
    Ping,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
#[serde(tag = "type")]
pub enum OutMsg {
    #[serde(rename = "connected")]
    Connected {
        device_id: String,
        name: String,
        os: String,
        sync_enabled: bool,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        ips: Vec<String>,
    },
    #[serde(rename = "ack")]
    Ack { ok: bool, clear_input: bool },
    #[serde(rename = "pong")]
    Pong { sync_enabled: bool },
    #[serde(rename = "sync_state")]
    SyncState { sync_enabled: bool },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qr_has_required_fields() {
        let q = QrPayload::new(
            "id".into(),
            "Mac".into(),
            "macos".into(),
            "192.168.1.2".into(),
            vec!["192.168.1.2".into(), "10.0.0.5".into()],
        );
        let v = serde_json::to_value(&q).unwrap();
        for key in ["v", "type", "device_id", "ip", "port", "name", "os"] {
            assert!(v.get(key).is_some(), "missing {key}");
        }
        assert_eq!(v["v"], 1);
        assert_eq!(v["type"], "agentpad");
        assert_eq!(v["port"], PORT);
        assert_eq!(v["ip"], "192.168.1.2");
        assert_eq!(v["ips"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn qr_switch_ip_keeps_ips() {
        let mut q = QrPayload::new(
            "id".into(),
            "Mac".into(),
            "macos".into(),
            "192.168.1.2".into(),
            vec!["192.168.1.2".into(), "10.0.0.5".into()],
        );
        q.ip = "10.0.0.5".into();
        let v = serde_json::to_value(&q).unwrap();
        assert_eq!(v["ip"], "10.0.0.5");
        assert_eq!(v["ips"][0], "192.168.1.2");
        assert_eq!(v["ips"][1], "10.0.0.5");
    }

    #[test]
    fn parse_client_messages() {
        let hello: InMsg =
            serde_json::from_str(r#"{"type":"hello","client_id":"c","client_name":"phone"}"#)
                .unwrap();
        assert!(matches!(hello, InMsg::Hello { .. }));
        let text: InMsg = serde_json::from_str(
            r#"{"type":"text","content":"hi","auto_enter":true,"send_mode":"submit"}"#,
        )
        .unwrap();
        assert!(matches!(
            text,
            InMsg::Text {
                send_mode: ref m,
                ..
            } if m == "submit"
        ));
        let key: InMsg =
            serde_json::from_str(r#"{"type":"key","key":"Escape","modifiers":[]}"#).unwrap();
        assert!(matches!(key, InMsg::Key { key: ref k, .. } if k == "Escape"));
        let ptr: InMsg =
            serde_json::from_str(r#"{"type":"pointer","dx":1.5,"dy":-2,"buttons":1,"wheel":0}"#)
                .unwrap();
        assert!(matches!(ptr, InMsg::Pointer { buttons: 1, .. }));
        assert!(serde_json::from_str::<InMsg>(r#"{"type":"undo"}"#).is_ok());
        assert!(serde_json::from_str::<InMsg>(r#"{"type":"ping"}"#).is_ok());
    }

    #[test]
    fn connected_fields() {
        let msg = OutMsg::Connected {
            device_id: "d".into(),
            name: "n".into(),
            os: "macos".into(),
            sync_enabled: true,
            ips: vec!["192.168.1.2".into()],
        };
        let v = serde_json::to_value(&msg).unwrap();
        for key in ["type", "device_id", "name", "os", "sync_enabled", "ips"] {
            assert!(v.get(key).is_some(), "missing {key}");
        }
        assert_eq!(v["type"], "connected");
    }
}
