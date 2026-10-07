use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;

use crate::handle::{self, Conn};
use crate::identity::Identity;
use crate::pairing::{self, Pairing};
use crate::protocol::{InMsg, OutMsg};

const POST_UPDATE_BIND_ATTEMPTS: usize = 50;
const POST_UPDATE_BIND_DELAY: std::time::Duration = std::time::Duration::from_millis(100);
const AUTH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const MAX_AUTH_BYTES: usize = 4 * 1024;
const MAX_UNAUTH: usize = 32;
const MAX_UNAUTH_PER_IP: usize = 4;
/// 入站消息和单帧共用。见 `ws_config`。
const WS_MAX_INBOUND: usize = 4 << 20;

/// tokio-tungstenite 0.26 的 `WebSocketStream` 只暴露 `get_config`，没有 `set_config`
/// （内部 `tungstenite::WebSocket::set_config` 够不着）。认证前后只能同一套上限。
/// 4MiB 盖住已有的 1MiB 首包测试和认证后的长文本，同时把默认 64MiB 消息 / 16MiB 帧压下来。
/// 读缓冲从默认 128KiB 降到 8KiB；写缓冲给不读数据的对端一个有限上限。
fn ws_config() -> WebSocketConfig {
    WebSocketConfig::default()
        .read_buffer_size(8 * 1024)
        .write_buffer_size(4 * 1024)
        .max_write_buffer_size(64 * 1024)
        .max_message_size(Some(WS_MAX_INBOUND))
        .max_frame_size(Some(WS_MAX_INBOUND))
}

pub struct AppState {
    /// `identity.secret` 只是启动时的值；当前密钥以 `secret()` 为准。
    pub identity: Identity,
    secret: Mutex<String>,
    pub pairing: Mutex<Pairing>,
    pub paused: AtomicBool,
    pub clients: Mutex<Vec<mpsc::UnboundedSender<OutMsg>>>,
    /// 还没完成首条认证的连接。认证结束就释放，长连接不占名额。
    unauth: Mutex<UnauthSlots>,
}

impl AppState {
    pub fn new(identity: Identity) -> Arc<Self> {
        Arc::new(Self {
            secret: Mutex::new(identity.secret.clone()),
            identity,
            pairing: Mutex::new(Pairing::default()),
            paused: AtomicBool::new(false),
            clients: Mutex::new(Vec::new()),
            unauth: Mutex::new(UnauthSlots::default()),
        })
    }

    /// 未认证名额。满了返回 None，调用方直接丢掉这条 TCP。
    fn try_reserve_unauth(self: &Arc<Self>, ip: IpAddr) -> Option<UnauthGuard> {
        let mut slots = self.unauth.lock().unwrap();
        // ponytail: 未认证只计全局 32 和单 IP 4，不做按秒或按字节的速率限制；名额不够再收紧。
        let ip_count = slots.by_ip.get(&ip).copied().unwrap_or(0);
        if slots.total >= MAX_UNAUTH || ip_count >= MAX_UNAUTH_PER_IP {
            return None;
        }
        slots.total += 1;
        slots.by_ip.insert(ip, ip_count + 1);
        drop(slots);
        Some(UnauthGuard {
            state: Arc::clone(self),
            ip,
        })
    }

    pub fn secret(&self) -> String {
        self.secret.lock().unwrap().clone()
    }

    /// 换新长期密钥并断开所有已配对连接；手机需重新扫码或输入配对码。
    pub fn reset_secret(&self) -> std::io::Result<()> {
        let secret = pairing::random_hex(32);
        crate::identity::save(&Identity {
            secret: secret.clone(),
            ..self.identity.clone()
        })?;
        self.replace_secret(secret);
        Ok(())
    }

    fn replace_secret(&self, secret: String) {
        *self.secret.lock().unwrap() = secret;
        self.clients.lock().unwrap().clear();
    }

    /// 首条文本必须是 hello（HMAC）或 pair（配对码）；成功时 pair 返回要下发的密钥。
    fn authenticate(&self, nonce: &str, first: InMsg) -> Result<Option<String>, &'static str> {
        match first {
            InMsg::Hello { auth, .. } => pairing::verify_auth(&self.secret(), nonce, &auth)
                .then_some(None)
                .ok_or("bad_auth"),
            InMsg::Pair { code, .. } => {
                let ok = self.pairing.lock().unwrap().try_code(&code);
                ok.then(|| Some(self.secret())).ok_or("bad_code")
            }
            _ => Err("unauthenticated"),
        }
    }

    pub fn sync_enabled(&self) -> bool {
        !self.paused.load(Ordering::SeqCst)
    }

    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::SeqCst);
        let msg = OutMsg::SyncState {
            sync_enabled: !paused,
        };
        let mut clients = self.clients.lock().unwrap();
        clients.retain(|tx| tx.send(msg.clone()).is_ok());
    }
}

pub async fn serve(state: Arc<AppState>, bind: SocketAddr) -> std::io::Result<SocketAddr> {
    let listener = TcpListener::bind(bind).await?;
    let addr = listener.local_addr()?;
    crate::logutil::write("connection listen ok");
    tokio::spawn(accept_loop(listener, state));
    Ok(addr)
}

pub async fn serve_with_retry(
    state: Arc<AppState>,
    bind: SocketAddr,
    post_update: bool,
) -> std::io::Result<SocketAddr> {
    let attempts = if post_update {
        POST_UPDATE_BIND_ATTEMPTS
    } else {
        1
    };
    for attempt in 0..attempts {
        match serve(state.clone(), bind).await {
            Err(e)
                if post_update
                    && e.kind() == std::io::ErrorKind::AddrInUse
                    && attempt + 1 < attempts =>
            {
                tokio::time::sleep(POST_UPDATE_BIND_DELAY).await;
            }
            result => return result,
        }
    }
    unreachable!("retry loop always returns")
}

#[derive(Default)]
struct UnauthSlots {
    total: usize,
    by_ip: HashMap<IpAddr, usize>,
}

/// 认证阶段占用的名额。任意退出路径都在 Drop 里归还。
struct UnauthGuard {
    state: Arc<AppState>,
    ip: IpAddr,
}

impl Drop for UnauthGuard {
    fn drop(&mut self) {
        let mut slots = self.state.unauth.lock().unwrap();
        slots.total = slots.total.saturating_sub(1);
        if let Some(count) = slots.by_ip.get_mut(&self.ip) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                slots.by_ip.remove(&self.ip);
            }
        }
    }
}

async fn accept_loop(listener: TcpListener, state: Arc<AppState>) {
    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                let Some(guard) = state.try_reserve_unauth(peer.ip()) else {
                    drop(stream);
                    crate::logutil::write("connection unauth limit");
                    continue;
                };
                crate::logutil::write("connection accept ok");
                let state = state.clone();
                tokio::spawn(async move {
                    if handle_socket(stream, state, guard, AUTH_TIMEOUT)
                        .await
                        .is_err()
                    {
                        crate::logutil::write("connection session failed");
                    }
                });
            }
            Err(_e) => {
                crate::logutil::write("connection accept failed");
            }
        }
    }
}

fn enable_pointer_tcp(stream: &TcpStream) {
    let _ = stream.set_nodelay(true);
}

fn enqueue_pointer(actions: Vec<handle::Action>) {
    #[cfg(test)]
    if let Some(tx) = pointer_capture().lock().unwrap().as_ref().cloned() {
        let _ = tx.send(actions);
        return;
    }
    static TX: std::sync::OnceLock<std::sync::mpsc::Sender<Vec<handle::Action>>> =
        std::sync::OnceLock::new();
    let tx = TX.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<Vec<handle::Action>>();
        std::thread::Builder::new()
            .name("agentpad-inject".into())
            .spawn(move || run_pointer_queue(rx, handle::apply_actions))
            .expect("inject thread");
        tx
    });
    let _ = tx.send(actions);
}

#[cfg(test)]
fn pointer_capture() -> &'static Mutex<Option<std::sync::mpsc::Sender<Vec<handle::Action>>>> {
    static SLOT: std::sync::OnceLock<Mutex<Option<std::sync::mpsc::Sender<Vec<handle::Action>>>>> =
        std::sync::OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

fn run_pointer_queue(
    rx: std::sync::mpsc::Receiver<Vec<handle::Action>>,
    mut apply: impl FnMut(&[handle::Action]),
) {
    let mut buttons_before_batch = 0;
    while let Ok(mut batch) = rx.recv() {
        while let Ok(more) = rx.try_recv() {
            merge_pointer_batch(&mut batch, more, buttons_before_batch);
        }
        apply(&batch);
        if let Some(handle::Action::Pointer { buttons, .. }) = batch.last() {
            buttons_before_batch = *buttons;
        }
    }
}

fn merge_pointer_batch(
    dst: &mut Vec<handle::Action>,
    more: Vec<handle::Action>,
    buttons_before_batch: u8,
) {
    for action in more {
        let buttons_before_tail = match &dst[..] {
            [.., handle::Action::Pointer { buttons, .. }, _] => Some(*buttons),
            [_] => Some(buttons_before_batch),
            _ => None,
        };
        match (&mut dst[..], action) {
            (
                [.., handle::Action::Pointer {
                    dx,
                    dy,
                    buttons,
                    wheel,
                }],
                handle::Action::Pointer {
                    dx: ddx,
                    dy: ddy,
                    buttons: btn,
                    wheel: wh,
                },
                // Native injection moves before changing buttons, so an edge packet
                // must not absorb motion or wheel input that happened after the edge.
            ) if *buttons == btn && buttons_before_tail == Some(btn) => {
                *dx += ddx;
                *dy += ddy;
                *wheel += wh;
            }
            (_, action) => dst.push(action),
        }
    }
}

async fn read_first_text(ws: &mut tokio_tungstenite::WebSocketStream<TcpStream>) -> Option<String> {
    while let Some(Ok(frame)) = ws.next().await {
        match frame {
            Message::Text(text) => return Some(text.to_string()),
            Message::Close(_) => return None,
            _ => {}
        }
    }
    None
}

async fn handle_socket(
    stream: TcpStream,
    state: Arc<AppState>,
    unauth: UnauthGuard,
    auth_timeout: std::time::Duration,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    enable_pointer_tcp(&stream);
    // 从 TCP 接入到首条认证消息读完，共用同一个截止时间。超时丢掉整个 future，连接一起关。
    let opened = tokio::time::timeout(auth_timeout, async {
        let mut ws = tokio_tungstenite::accept_async_with_config(stream, Some(ws_config())).await?;
        let nonce = pairing::random_hex(16);
        let challenge = OutMsg::Challenge {
            nonce: nonce.clone(),
            device_id: state.identity.device_id.clone(),
        };
        ws.send(Message::Text(serde_json::to_string(&challenge)?.into()))
            .await?;
        let first = read_first_text(&mut ws).await;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>((ws, nonce, first))
    })
    .await;
    let (mut ws, nonce, first) = match opened {
        Ok(Ok(ready)) => ready,
        Ok(Err(err)) => return Err(err),
        Err(_elapsed) => {
            crate::logutil::write("connection auth timeout");
            return Ok(());
        }
    };
    let first = first
        .filter(|text| text.len() <= MAX_AUTH_BYTES)
        .and_then(|text| serde_json::from_str::<InMsg>(&text).ok());
    let (tx, mut rx) = mpsc::unbounded_channel::<OutMsg>();
    let verdict = first.map_or(Err("unauthenticated"), |msg| {
        let mut clients = state.clients.lock().unwrap();
        let verdict = state.authenticate(&nonce, msg);
        if verdict.is_ok() {
            clients.retain(|t| !t.is_closed());
            clients.push(tx);
        }
        verdict
    });
    let secret = match verdict {
        Ok(secret) => {
            drop(unauth);
            secret
        }
        Err(reason) => {
            crate::logutil::write("connection auth failed");
            let failed = OutMsg::AuthFailed { reason };
            let _ = ws
                .send(Message::Text(serde_json::to_string(&failed)?.into()))
                .await;
            let _ = ws.close(None).await;
            return Ok(());
        }
    };
    crate::logutil::write(if secret.is_some() {
        "connection pair ok"
    } else {
        "connection auth ok"
    });
    let ips: Vec<String> = crate::net::list_nics().into_iter().map(|n| n.ip).collect();
    let connected = OutMsg::Connected {
        device_id: state.identity.device_id.clone(),
        name: state.identity.name.clone(),
        os: agentpad_input::os().to_string(),
        sync_enabled: state.sync_enabled(),
        ips,
        secret,
    };
    ws.send(Message::Text(serde_json::to_string(&connected)?.into()))
        .await?;

    let (mut sink, mut source) = ws.split();
    let mut conn = Conn::default();
    // 内层 future 吸收读写上的 `?`，无论 break、出错还是被重置密钥踢掉，外层都先抬起按键。
    let session = async {
        loop {
            tokio::select! {
                out = rx.recv() => {
                    let Some(msg) = out else { break; };
                    sink.send(Message::Text(serde_json::to_string(&msg)?.into())).await?;
                    crate::logutil::write("message send ok");
                }
                incoming = source.next() => {
                    let Some(frame) = incoming else { break; };
                    let Message::Text(text) = frame? else { continue; };
                    let Ok(msg) = serde_json::from_str::<InMsg>(&text) else {
                        crate::logutil::write("message parse failed");
                        continue;
                    };
                    let paused = state.paused.load(Ordering::SeqCst);
                    crate::logutil::operation(crate::logutil::input_category(&msg), false, !paused || matches!(msg, InMsg::Hello { .. } | InMsg::Ping));
                    let pointer = matches!(msg, InMsg::Pointer { .. });
                    let (replies, actions) = handle::handle(paused, state.sync_enabled(), &mut conn, msg);
                    if !actions.is_empty() {
                        if actions
                            .iter()
                            .all(|a| matches!(a, handle::Action::Pointer { .. }))
                        {
                            enqueue_pointer(actions);
                        } else {
                            tokio::task::block_in_place(|| handle::apply_actions(&actions));
                        }
                    }
                    for r in replies {
                        sink.send(Message::Text(serde_json::to_string(&r)?.into())).await?;
                        if !pointer {
                            crate::logutil::write("message send ok");
                        }
                    }
                }
            }
        }
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    if let Some(action) = handle::release_stuck_pointer(&mut conn) {
        enqueue_pointer(vec![action]);
    }
    session?;
    crate::logutil::write("connection session closed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::Identity;
    use futures_util::StreamExt;
    use tokio_tungstenite::connect_async;

    #[test]
    fn merges_backlogged_pointer_batches() {
        let mut batch = vec![handle::Action::Pointer {
            dx: 1.0,
            dy: 2.0,
            buttons: 0,
            wheel: 0,
        }];
        merge_pointer_batch(
            &mut batch,
            vec![handle::Action::Pointer {
                dx: 3.0,
                dy: -1.0,
                buttons: 0,
                wheel: 4,
            }],
            0,
        );
        assert_eq!(
            batch,
            vec![handle::Action::Pointer {
                dx: 4.0,
                dy: 1.0,
                buttons: 0,
                wheel: 4,
            }]
        );
    }

    #[test]
    fn keeps_click_button_edges_when_merging_backlog() {
        let mut batch = vec![handle::Action::Pointer {
            dx: 0.0,
            dy: 0.0,
            buttons: 1,
            wheel: 0,
        }];
        merge_pointer_batch(
            &mut batch,
            vec![handle::Action::Pointer {
                dx: 0.0,
                dy: 0.0,
                buttons: 0,
                wheel: 0,
            }],
            0,
        );
        assert_eq!(
            batch,
            vec![
                handle::Action::Pointer {
                    dx: 0.0,
                    dy: 0.0,
                    buttons: 1,
                    wheel: 0,
                },
                handle::Action::Pointer {
                    dx: 0.0,
                    dy: 0.0,
                    buttons: 0,
                    wheel: 0,
                },
            ]
        );
    }

    fn pointer(dx: f64, buttons: u8, wheel: i32) -> handle::Action {
        handle::Action::Pointer {
            dx,
            dy: 0.0,
            buttons,
            wheel,
        }
    }

    #[test]
    fn keeps_drag_start_before_backlogged_motion() {
        let mut batch = vec![pointer(0.0, 1, 0)];
        merge_pointer_batch(&mut batch, vec![pointer(4.0, 1, 0)], 0);
        assert_eq!(batch, vec![pointer(0.0, 1, 0), pointer(4.0, 1, 0)]);
    }

    #[test]
    fn keeps_release_before_backlogged_hover() {
        let mut batch = vec![pointer(0.0, 1, 0), pointer(0.0, 0, 0)];
        merge_pointer_batch(&mut batch, vec![pointer(4.0, 0, 0)], 0);
        assert_eq!(
            batch,
            vec![pointer(0.0, 1, 0), pointer(0.0, 0, 0), pointer(4.0, 0, 0)]
        );
    }

    #[test]
    fn keeps_wheel_after_backlogged_button_edge() {
        let mut batch = vec![pointer(0.0, 1, 0)];
        merge_pointer_batch(&mut batch, vec![pointer(0.0, 1, 3)], 0);
        assert_eq!(batch, vec![pointer(0.0, 1, 0), pointer(0.0, 1, 3)]);
    }

    #[test]
    fn still_merges_backlogged_drag_after_button_edge() {
        let mut batch = vec![pointer(0.0, 1, 0), pointer(1.0, 1, 1)];
        merge_pointer_batch(&mut batch, vec![pointer(2.0, 1, 2), pointer(3.0, 1, 3)], 0);
        assert_eq!(batch, vec![pointer(0.0, 1, 0), pointer(6.0, 1, 6)]);
    }

    #[test]
    fn keeps_release_edge_at_start_of_next_batch() {
        let mut batch = vec![pointer(0.0, 0, 0)];
        merge_pointer_batch(&mut batch, vec![pointer(4.0, 0, 0)], 1);
        assert_eq!(batch, vec![pointer(0.0, 0, 0), pointer(4.0, 0, 0)]);
    }

    #[test]
    fn merges_held_drag_across_batches() {
        let mut batch = vec![pointer(1.0, 1, 1)];
        merge_pointer_batch(&mut batch, vec![pointer(2.0, 1, 2)], 1);
        assert_eq!(batch, vec![pointer(3.0, 1, 3)]);
    }

    #[test]
    fn pointer_worker_keeps_edges_after_stalled_injection() {
        let (tx, rx) = std::sync::mpsc::channel();
        let (observed_tx, observed_rx) = std::sync::mpsc::channel();
        let (resume_tx, resume_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let mut first = true;
            run_pointer_queue(rx, |batch| {
                observed_tx.send(batch.to_vec()).unwrap();
                if first {
                    first = false;
                    resume_rx.recv().unwrap();
                }
            });
        });
        let timeout = std::time::Duration::from_secs(5);
        tx.send(vec![pointer(0.0, 1, 0)]).unwrap();
        assert_eq!(
            observed_rx.recv_timeout(timeout).unwrap(),
            vec![pointer(0.0, 1, 0)]
        );
        tx.send(vec![pointer(0.0, 0, 0)]).unwrap();
        tx.send(vec![pointer(4.0, 0, 1)]).unwrap();
        tx.send(vec![pointer(2.0, 0, 2)]).unwrap();
        drop(tx);
        resume_tx.send(()).unwrap();
        assert_eq!(
            observed_rx.recv_timeout(timeout).unwrap(),
            vec![pointer(0.0, 0, 0), pointer(6.0, 0, 3)]
        );
        worker.join().unwrap();
    }

    #[tokio::test]
    async fn ordinary_second_instance_does_not_retry() {
        let held = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = held.local_addr().unwrap();
        let state = AppState::new(test_identity());
        let err = serve_with_retry(state, addr, false).await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AddrInUse);
    }

    #[tokio::test]
    async fn post_update_waits_for_previous_listener() {
        let held = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = held.local_addr().unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(150));
            drop(held);
        });
        let state = AppState::new(test_identity());
        let bound = serve_with_retry(state, addr, true).await.unwrap();
        release.join().unwrap();
        assert_eq!(bound, addr);
    }

    fn test_identity() -> Identity {
        Identity {
            device_id: "dev-1".into(),
            name: "TestMac".into(),
            secret: "s3cret".into(),
        }
    }

    type Client = tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >;

    async fn next_json(ws: &mut Client) -> Option<serde_json::Value> {
        loop {
            match ws.next().await? {
                Ok(Message::Text(text)) => return Some(serde_json::from_str(&text).unwrap()),
                Ok(Message::Close(_)) | Err(_) => return None,
                Ok(_) => {}
            }
        }
    }

    /// 连接并以首条消息 `first(nonce)` 回应挑战，返回连接与服务端回复。
    async fn open(
        addr: SocketAddr,
        first: impl FnOnce(&str) -> String,
    ) -> (Client, Option<serde_json::Value>) {
        let (mut ws, _) = connect_async(format!("ws://{addr}")).await.unwrap();
        let challenge = next_json(&mut ws).await.unwrap();
        assert_eq!(challenge["type"], "challenge");
        assert_eq!(challenge["device_id"], "dev-1");
        let nonce = challenge["nonce"].as_str().unwrap().to_string();
        assert_eq!(nonce.len(), 32);
        ws.send(Message::Text(first(&nonce).into())).await.unwrap();
        let reply = next_json(&mut ws).await;
        (ws, reply)
    }

    fn hello(secret: &str, nonce: &str) -> String {
        format!(
            r#"{{"type":"hello","client_id":"c","client_name":"Android","auth":"{}"}}"#,
            pairing::auth_tag(secret, nonce)
        )
    }

    fn pair(code: &str) -> String {
        format!(r#"{{"type":"pair","client_id":"c","client_name":"Android","code":"{code}"}}"#)
    }

    #[tokio::test]
    async fn unauthenticated_clients_are_rejected_before_any_input() {
        let state = AppState::new(test_identity());
        let addr = serve(state.clone(), "127.0.0.1:0".parse().unwrap())
            .await
            .unwrap();
        for (first, reason) in [
            (
                r#"{"type":"text","content":"x","auto_enter":true,"send_mode":"submit"}"#
                    .to_string(),
                "unauthenticated",
            ),
            (r#"{"type":"ping"}"#.to_string(), "unauthenticated"),
            (hello("wrong", "n"), "bad_auth"),
            (pair("0000"), "bad_code"),
        ] {
            let (mut ws, reply) = open(addr, |_| first).await;
            let reply = reply.unwrap();
            assert_eq!(reply["type"], "auth_failed");
            assert_eq!(reply["reason"], reason);
            assert!(next_json(&mut ws).await.is_none());
        }
        assert!(state.clients.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn authentication_first_message_has_4kib_limit() {
        let state = AppState::new(test_identity());
        let addr = serve(state, "127.0.0.1:0".parse().unwrap()).await.unwrap();
        for size in [4096, 4097, 1 << 20] {
            let (mut ws, reply) = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                open(addr, |nonce| {
                    let mut text = hello("s3cret", nonce);
                    text.push_str(&" ".repeat(size - text.len()));
                    text
                }),
            )
            .await
            .unwrap();
            let reply = reply.unwrap();
            if size == 4096 {
                assert_eq!(reply["type"], "connected");
            } else {
                assert_eq!(reply["type"], "auth_failed", "size {size}");
                assert_eq!(reply["reason"], "unauthenticated");
                assert!(next_json(&mut ws).await.is_none());
            }
        }
    }

    #[tokio::test]
    async fn authenticated_long_text_is_not_subject_to_auth_limit() {
        let state = AppState::new(test_identity());
        state.set_paused(true);
        let addr = serve(state, "127.0.0.1:0".parse().unwrap()).await.unwrap();
        let (mut ws, _connected) = open(addr, |nonce| hello("s3cret", nonce)).await;
        let text = serde_json::json!({
            "type": "text",
            "content": "文".repeat(32_768),
            "auto_enter": false,
            "send_mode": "submit",
        })
        .to_string();
        assert!(text.len() > 4096);
        ws.send(Message::Text(text.into())).await.unwrap();
        let ack = tokio::time::timeout(std::time::Duration::from_secs(5), next_json(&mut ws))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ack["type"], "ack");
        assert_eq!(ack["ok"], false);
        ws.send(Message::Text(r#"{"type":"ping"}"#.into()))
            .await
            .unwrap();
        assert_eq!(next_json(&mut ws).await.unwrap()["type"], "pong");
    }

    #[tokio::test]
    async fn secret_reset_rejects_pending_old_key_authentication() {
        let state = AppState::new(test_identity());
        let addr = serve(state.clone(), "127.0.0.1:0".parse().unwrap())
            .await
            .unwrap();
        let (mut pending, _) = connect_async(format!("ws://{addr}")).await.unwrap();
        let challenge = next_json(&mut pending).await.unwrap();
        let nonce = challenge["nonce"].as_str().unwrap();
        state.replace_secret("new-secret".into());
        pending
            .send(Message::Text(hello("s3cret", nonce).into()))
            .await
            .unwrap();
        let failed = next_json(&mut pending).await.unwrap();
        assert_eq!(failed["type"], "auth_failed");
        assert_eq!(failed["reason"], "bad_auth");
        assert!(next_json(&mut pending).await.is_none());
        let (_ws, reply) = open(addr, |nonce| hello("new-secret", nonce)).await;
        assert_eq!(reply.unwrap()["type"], "connected");
    }

    #[tokio::test]
    async fn pairing_code_hands_out_secret_once_and_reset_disconnects() {
        let state = AppState::new(test_identity());
        let addr = serve(state.clone(), "127.0.0.1:0".parse().unwrap())
            .await
            .unwrap();
        state.pairing.lock().unwrap().set_window_open(true);
        let code = state.pairing.lock().unwrap().code().unwrap().to_string();
        let (_ws, reply) = open(addr, |_| pair(&code)).await;
        let reply = reply.unwrap();
        assert_eq!(reply["type"], "connected");
        assert_eq!(reply["secret"], "s3cret");

        let (_, reply) = open(addr, |_| pair(&code)).await;
        assert_eq!(reply.unwrap()["reason"], "bad_code");

        let (mut paired, reply) = open(addr, |n| hello("s3cret", n)).await;
        let reply = reply.unwrap();
        assert_eq!(reply["type"], "connected");
        assert!(reply.get("secret").is_none());

        state.replace_secret(pairing::random_hex(32));
        assert!(next_json(&mut paired).await.is_none());
        let (_, reply) = open(addr, |n| hello("s3cret", n)).await;
        assert_eq!(reply.unwrap()["reason"], "bad_auth");
        let secret = state.secret();
        let (_, reply) = open(addr, |n| hello(&secret, n)).await;
        assert_eq!(reply.unwrap()["type"], "connected");
    }

    #[tokio::test]
    async fn connect_receives_connected() {
        let state = AppState::new(test_identity());
        let addr = serve(state, "127.0.0.1:0".parse().unwrap()).await.unwrap();
        let (_ws, v) = open(addr, |n| hello("s3cret", n)).await;
        let v = v.unwrap();
        assert_eq!(v["type"], "connected");
        assert_eq!(v["device_id"], "dev-1");
        assert_eq!(v["name"], "TestMac");
        assert_eq!(v["os"], agentpad_input::os());
        assert_eq!(v["sync_enabled"], true);
    }

    #[tokio::test]
    async fn ping_pong_and_paused_ack() {
        let state = AppState::new(test_identity());
        state.set_paused(true);
        let addr = serve(state, "127.0.0.1:0".parse().unwrap()).await.unwrap();
        let (mut ws, _connected) = open(addr, |n| hello("s3cret", n)).await;
        ws.send(Message::Text(r#"{"type":"ping"}"#.into()))
            .await
            .unwrap();
        let pong = ws.next().await.unwrap().unwrap();
        let Message::Text(text) = pong else { panic!() };
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["type"], "pong");
        assert_eq!(v["sync_enabled"], false);

        ws.send(Message::Text(
            r#"{"type":"text","content":"x","auto_enter":false,"send_mode":"submit"}"#.into(),
        ))
        .await
        .unwrap();
        let ack = ws.next().await.unwrap().unwrap();
        let Message::Text(text) = ack else { panic!() };
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["ok"], false);
    }

    #[test]
    fn unauth_slots_cap_per_ip_and_global_then_release() {
        let state = AppState::new(test_identity());
        let ip_a = IpAddr::from([192, 0, 2, 1]);
        let mut held = Vec::new();
        for _ in 0..MAX_UNAUTH_PER_IP {
            held.push(state.try_reserve_unauth(ip_a).unwrap());
        }
        assert!(state.try_reserve_unauth(ip_a).is_none());
        let ip_b = IpAddr::from([192, 0, 2, 2]);
        held.push(state.try_reserve_unauth(ip_b).unwrap());

        let mut rest = Vec::new();
        for n in 0..(MAX_UNAUTH - held.len()) {
            let ip = IpAddr::from([198, 51, 100, (n + 1) as u8]);
            rest.push(state.try_reserve_unauth(ip).unwrap());
        }
        assert_eq!(state.unauth.lock().unwrap().total, MAX_UNAUTH);
        assert!(state
            .try_reserve_unauth(IpAddr::from([203, 0, 113, 1]))
            .is_none());

        held.remove(0);
        assert!(state.try_reserve_unauth(ip_a).is_some());
        rest.pop();
        assert!(state
            .try_reserve_unauth(IpAddr::from([203, 0, 113, 2]))
            .is_some());
        drop(held);
        drop(rest);
        assert_eq!(state.unauth.lock().unwrap().total, 0);
        assert!(state.unauth.lock().unwrap().by_ip.is_empty());
    }

    async fn wait_unauth(state: &AppState, total: usize) {
        let wait = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while state.unauth.lock().unwrap().total != total {
                tokio::task::yield_now().await;
            }
        })
        .await;
        assert!(
            wait.is_ok(),
            "unauth total stayed {}",
            state.unauth.lock().unwrap().total
        );
    }

    async fn tcp_closed_within(stream: &mut TcpStream, limit: std::time::Duration) -> bool {
        use tokio::io::AsyncReadExt;
        let mut buf = [0u8; 8];
        matches!(
            tokio::time::timeout(limit, stream.read(&mut buf)).await,
            Ok(Ok(0)) | Ok(Err(_))
        )
    }

    #[tokio::test]
    async fn unauth_cap_rejects_extra_tcp_and_releases_when_it_ends() {
        let state = AppState::new(test_identity());
        let addr = serve(state.clone(), "127.0.0.1:0".parse().unwrap())
            .await
            .unwrap();

        let (mut bad, reply) = open(addr, |_| r#"{"type":"ping"}"#.to_string()).await;
        assert_eq!(reply.unwrap()["reason"], "unauthenticated");
        assert!(next_json(&mut bad).await.is_none());
        wait_unauth(&state, 0).await;

        let (mut authed, reply) = open(addr, |n| hello("s3cret", n)).await;
        assert_eq!(reply.unwrap()["type"], "connected");
        assert_eq!(state.unauth.lock().unwrap().total, 0);

        let mut hangs = Vec::new();
        for _ in 0..MAX_UNAUTH_PER_IP {
            hangs.push(TcpStream::connect(addr).await.unwrap());
        }
        wait_unauth(&state, MAX_UNAUTH_PER_IP).await;

        let mut extra = TcpStream::connect(addr).await.unwrap();
        assert!(tcp_closed_within(&mut extra, std::time::Duration::from_secs(2)).await);
        assert_eq!(state.unauth.lock().unwrap().total, MAX_UNAUTH_PER_IP);

        drop(hangs);
        wait_unauth(&state, 0).await;
        authed
            .send(Message::Text(r#"{"type":"ping"}"#.into()))
            .await
            .unwrap();
        assert_eq!(next_json(&mut authed).await.unwrap()["type"], "pong");
    }

    #[tokio::test]
    async fn stalled_handshake_closes_at_injected_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let state = AppState::new(test_identity());
        let server_state = state.clone();
        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            let guard = server_state.try_reserve_unauth(peer.ip()).unwrap();
            let started = std::time::Instant::now();
            let result = super::handle_socket(
                stream,
                server_state.clone(),
                guard,
                std::time::Duration::from_millis(300),
            )
            .await;
            (
                started.elapsed(),
                result,
                server_state.unauth.lock().unwrap().total,
            )
        });
        let mut client = TcpStream::connect(addr).await.unwrap();
        assert!(!tcp_closed_within(&mut client, std::time::Duration::from_millis(80)).await);
        assert!(tcp_closed_within(&mut client, std::time::Duration::from_secs(2)).await);
        let (elapsed, result, left) =
            tokio::time::timeout(std::time::Duration::from_secs(3), server)
                .await
                .unwrap()
                .unwrap();
        assert!(result.is_ok());
        assert_eq!(left, 0);
        assert!(elapsed < std::time::Duration::from_secs(2));
    }

    #[tokio::test]
    async fn silent_client_after_challenge_closes_at_same_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let state = AppState::new(test_identity());
        let server_state = state.clone();
        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            let guard = server_state.try_reserve_unauth(peer.ip()).unwrap();
            super::handle_socket(
                stream,
                server_state,
                guard,
                std::time::Duration::from_millis(800),
            )
            .await
        });
        let (mut ws, _) = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            connect_async(format!("ws://{addr}")),
        )
        .await
        .unwrap()
        .unwrap();
        let challenge = tokio::time::timeout(std::time::Duration::from_secs(2), next_json(&mut ws))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(challenge["type"], "challenge");
        let early =
            tokio::time::timeout(std::time::Duration::from_millis(150), next_json(&mut ws)).await;
        assert!(early.is_err(), "closed before the deadline");
        let later = tokio::time::timeout(std::time::Duration::from_secs(2), next_json(&mut ws))
            .await
            .unwrap();
        assert!(later.is_none());
        assert!(server.await.unwrap().is_ok());
    }

    #[tokio::test]
    async fn oversized_frame_is_rejected_before_the_body() {
        use tokio::io::AsyncWriteExt;
        let state = AppState::new(test_identity());
        let addr = serve(state, "127.0.0.1:0".parse().unwrap()).await.unwrap();
        let (mut ws, _) = connect_async(format!("ws://{addr}")).await.unwrap();
        let challenge = next_json(&mut ws).await.unwrap();
        assert_eq!(challenge["type"], "challenge");
        let mut header = [0u8; 14];
        header[0] = 0x81;
        header[1] = 0xFF;
        header[2..10].copy_from_slice(&(WS_MAX_INBOUND as u64 + 1).to_be_bytes());
        {
            let tokio_tungstenite::MaybeTlsStream::Plain(tcp) = ws.get_mut() else {
                panic!("expected plain tcp");
            };
            tcp.write_all(&header).await.unwrap();
            tcp.flush().await.unwrap();
        }
        let reply = tokio::time::timeout(std::time::Duration::from_secs(2), next_json(&mut ws))
            .await
            .expect("oversized frame was not rejected");
        match reply {
            None => {}
            Some(value) => {
                assert_eq!(value["type"], "auth_failed");
                assert_eq!(value["reason"], "unauthenticated");
            }
        }
    }

    #[tokio::test]
    async fn accepted_socket_disables_nagle() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            super::enable_pointer_tcp(&stream);
            stream.nodelay().unwrap()
        });
        let _client = tokio::net::TcpStream::connect(addr).await.unwrap();
        assert!(server.await.unwrap());
    }

    fn button_up() -> handle::Action {
        handle::Action::Pointer {
            dx: 0.0,
            dy: 0.0,
            buttons: 0,
            wheel: 0,
        }
    }

    fn pointer_json(dx: f64, dy: f64, buttons: u8, wheel: i32) -> String {
        format!(r#"{{"type":"pointer","dx":{dx},"dy":{dy},"buttons":{buttons},"wheel":{wheel}}}"#)
    }

    /// 把指针动作拦在注入线程外面，测试里不会碰到真的键鼠。
    struct PointerCapture {
        rx: std::sync::mpsc::Receiver<Vec<handle::Action>>,
        _gate: tokio::sync::MutexGuard<'static, ()>,
    }

    impl PointerCapture {
        async fn install() -> Self {
            static GATE: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
            let gate = GATE
                .get_or_init(|| tokio::sync::Mutex::new(()))
                .lock()
                .await;
            let (tx, rx) = std::sync::mpsc::channel();
            *super::pointer_capture().lock().unwrap() = Some(tx);
            Self { rx, _gate: gate }
        }

        async fn recv(&self) -> Vec<handle::Action> {
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
            loop {
                match self.rx.try_recv() {
                    Ok(batch) => return batch,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        panic!("pointer capture closed")
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        if tokio::time::Instant::now() >= deadline {
                            panic!("timed out waiting for pointer action");
                        }
                        tokio::task::yield_now().await;
                    }
                }
            }
        }

        /// 这段时间里让出运行时，确认没有再入队。阻塞等待会冻住当前线程上的服务端任务。
        async fn quiet(&self, limit: std::time::Duration) -> bool {
            let start = tokio::time::Instant::now();
            while start.elapsed() < limit {
                if self.rx.try_recv().is_ok() {
                    return false;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            self.rx.try_recv().is_err()
        }
    }

    impl Drop for PointerCapture {
        fn drop(&mut self) {
            *super::pointer_capture().lock().unwrap() = None;
        }
    }

    async fn authed_client(state: Arc<AppState>) -> (SocketAddr, Client) {
        let addr = serve(state, "127.0.0.1:0".parse().unwrap()).await.unwrap();
        let (ws, reply) = open(addr, |n| hello("s3cret", n)).await;
        assert_eq!(reply.unwrap()["type"], "connected");
        (addr, ws)
    }

    #[tokio::test]
    async fn disconnect_after_press_enqueues_one_release() {
        let capture = PointerCapture::install().await;
        let state = AppState::new(test_identity());
        let (_addr, mut ws) = authed_client(state).await;
        ws.send(Message::Text(pointer_json(1.5, -2.0, 1, 0).into()))
            .await
            .unwrap();
        assert_eq!(
            capture.recv().await,
            vec![handle::Action::Pointer {
                dx: 1.5,
                dy: -2.0,
                buttons: 1,
                wheel: 0,
            }]
        );
        drop(ws);
        assert_eq!(capture.recv().await, vec![button_up()]);
        assert!(capture.quiet(std::time::Duration::from_millis(200)).await);
    }

    #[tokio::test]
    async fn button_up_then_disconnect_does_not_release_again() {
        let capture = PointerCapture::install().await;
        let state = AppState::new(test_identity());
        let (_addr, mut ws) = authed_client(state).await;
        ws.send(Message::Text(pointer_json(1.0, 0.0, 1, 0).into()))
            .await
            .unwrap();
        assert_eq!(capture.recv().await, vec![pointer(1.0, 1, 0)]);
        ws.send(Message::Text(pointer_json(3.0, 4.0, 0, 0).into()))
            .await
            .unwrap();
        assert_eq!(
            capture.recv().await,
            vec![handle::Action::Pointer {
                dx: 3.0,
                dy: 4.0,
                buttons: 0,
                wheel: 0,
            }]
        );
        drop(ws);
        assert!(capture.quiet(std::time::Duration::from_millis(200)).await);
    }

    #[tokio::test]
    async fn disconnect_without_press_enqueues_nothing() {
        let capture = PointerCapture::install().await;
        let state = AppState::new(test_identity());
        let (_addr, ws) = authed_client(state).await;
        drop(ws);
        assert!(capture.quiet(std::time::Duration::from_millis(300)).await);
    }

    #[tokio::test]
    async fn pause_forwards_button_up_and_disconnect_does_not_repeat_it() {
        let capture = PointerCapture::install().await;
        let state = AppState::new(test_identity());
        let (_addr, mut ws) = authed_client(state.clone()).await;
        ws.send(Message::Text(pointer_json(1.0, 0.0, 1, 0).into()))
            .await
            .unwrap();
        assert_eq!(capture.recv().await, vec![pointer(1.0, 1, 0)]);
        state.set_paused(true);
        ws.send(Message::Text(pointer_json(8.0, 9.0, 1, 2).into()))
            .await
            .unwrap();
        assert!(capture.quiet(std::time::Duration::from_millis(200)).await);
        ws.send(Message::Text(pointer_json(8.0, 9.0, 0, 4).into()))
            .await
            .unwrap();
        assert_eq!(capture.recv().await, vec![button_up()]);
        drop(ws);
        assert!(capture.quiet(std::time::Duration::from_millis(200)).await);
    }

    #[tokio::test]
    async fn secret_reset_and_protocol_error_still_release() {
        use tokio::io::AsyncWriteExt;
        let capture = PointerCapture::install().await;
        let state = AppState::new(test_identity());
        let (_addr, mut ws) = authed_client(state.clone()).await;
        ws.send(Message::Text(pointer_json(2.0, 0.0, 1, 0).into()))
            .await
            .unwrap();
        assert_eq!(capture.recv().await, vec![pointer(2.0, 1, 0)]);
        state.replace_secret("rotated-secret".into());
        assert_eq!(capture.recv().await, vec![button_up()]);

        let (_addr, mut ws) = authed_client(AppState::new(test_identity())).await;
        ws.send(Message::Text(pointer_json(2.0, 0.0, 5, 0).into()))
            .await
            .unwrap();
        assert_eq!(capture.recv().await, vec![pointer(2.0, 5, 0)]);
        {
            let tokio_tungstenite::MaybeTlsStream::Plain(tcp) = ws.get_mut() else {
                panic!("expected plain tcp");
            };
            // 孤立的 continuation，服务端读帧会出错，走 `?` 退出。
            tcp.write_all(&[0x80, 0x80, 0, 0, 0, 0]).await.unwrap();
            tcp.flush().await.unwrap();
        }
        assert_eq!(capture.recv().await, vec![button_up()]);
    }

    #[test]
    fn queued_release_reaches_fake_apply_intact() {
        let (tx, rx) = std::sync::mpsc::channel();
        let (seen_tx, seen_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            run_pointer_queue(rx, |batch| {
                seen_tx.send(batch.to_vec()).unwrap();
            });
        });
        let down = pointer(4.0, 1, 0);
        let up = button_up();
        tx.send(vec![down.clone()]).unwrap();
        tx.send(vec![up.clone()]).unwrap();
        drop(tx);
        let mut applied = Vec::new();
        while let Ok(batch) = seen_rx.recv_timeout(std::time::Duration::from_secs(2)) {
            applied.extend(batch);
        }
        worker.join().unwrap();
        assert_eq!(applied, vec![down, up]);
    }
}
