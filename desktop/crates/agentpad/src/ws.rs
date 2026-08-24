use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use crate::handle::{self, Conn};
use crate::identity::Identity;
use crate::protocol::{InMsg, OutMsg};

const POST_UPDATE_BIND_ATTEMPTS: usize = 50;
const POST_UPDATE_BIND_DELAY: std::time::Duration = std::time::Duration::from_millis(100);

pub struct AppState {
    pub identity: Identity,
    pub paused: AtomicBool,
    pub clients: Mutex<Vec<mpsc::UnboundedSender<OutMsg>>>,
}

impl AppState {
    pub fn new(identity: Identity) -> Arc<Self> {
        Arc::new(Self {
            identity,
            paused: AtomicBool::new(false),
            clients: Mutex::new(Vec::new()),
        })
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
    crate::logutil::write(&format!("ws listen {addr}"));
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

async fn accept_loop(listener: TcpListener, state: Arc<AppState>) {
    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                crate::logutil::write(&format!("ws accept {peer}"));
                let state = state.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_socket(stream, state).await {
                        crate::logutil::write(&format!("ws session: {e}"));
                    }
                });
            }
            Err(e) => {
                crate::logutil::write(&format!("ws accept err: {e}"));
            }
        }
    }
}

fn enable_pointer_tcp(stream: &TcpStream) {
    let _ = stream.set_nodelay(true);
}

fn enqueue_pointer(actions: Vec<handle::Action>) {
    static TX: std::sync::OnceLock<std::sync::mpsc::Sender<Vec<handle::Action>>> =
        std::sync::OnceLock::new();
    let tx = TX.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<Vec<handle::Action>>();
        std::thread::Builder::new()
            .name("agentpad-inject".into())
            .spawn(move || {
                while let Ok(mut batch) = rx.recv() {
                    while let Ok(more) = rx.try_recv() {
                        merge_pointer_batch(&mut batch, more);
                    }
                    handle::apply_actions(&batch);
                }
            })
            .expect("inject thread");
        tx
    });
    let _ = tx.send(actions);
}

fn merge_pointer_batch(dst: &mut Vec<handle::Action>, more: Vec<handle::Action>) {
    for action in more {
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
            // Clicks are button edges across packets (1 then 0). Overwriting
            // buttons while coalescing motion drops the press entirely — and
            // Windows inject is slower, so backlog merges hit clicks more often.
            ) if *buttons == btn => {
                *dx += ddx;
                *dy += ddy;
                *wheel += wh;
            }
            (_, action) => dst.push(action),
        }
    }
}

async fn handle_socket(
    stream: TcpStream,
    state: Arc<AppState>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    enable_pointer_tcp(&stream);
    let mut ws = tokio_tungstenite::accept_async(stream).await?;
    let ips: Vec<String> = crate::net::list_nics().into_iter().map(|n| n.ip).collect();
    let connected = OutMsg::Connected {
        device_id: state.identity.device_id.clone(),
        name: state.identity.name.clone(),
        os: agentpad_input::os().to_string(),
        sync_enabled: state.sync_enabled(),
        ips,
    };
    ws.send(Message::Text(serde_json::to_string(&connected)?.into()))
        .await?;

    let (tx, mut rx) = mpsc::unbounded_channel::<OutMsg>();
    state.clients.lock().unwrap().push(tx);

    let (mut sink, mut source) = ws.split();
    let mut conn = Conn::default();

    loop {
        tokio::select! {
            out = rx.recv() => {
                let Some(msg) = out else { break; };
                sink.send(Message::Text(serde_json::to_string(&msg)?.into())).await?;
            }
            incoming = source.next() => {
                let Some(frame) = incoming else { break; };
                let Message::Text(text) = frame? else { continue; };
                let Ok(msg) = serde_json::from_str::<InMsg>(&text) else {
                    crate::logutil::write(&format!("bad msg {text}"));
                    continue;
                };
                if !matches!(msg, InMsg::Pointer { .. } | InMsg::Ping) {
                    crate::logutil::write(&format!("in {msg:?}"));
                }
                let paused = state.paused.load(Ordering::SeqCst);
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
                }
            }
        }
    }
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

    #[tokio::test]
    async fn ordinary_second_instance_does_not_retry() {
        let held = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = held.local_addr().unwrap();
        let state = AppState::new(Identity {
            device_id: "dev-1".into(),
            name: "TestMac".into(),
        });
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
        let state = AppState::new(Identity {
            device_id: "dev-1".into(),
            name: "TestMac".into(),
        });
        let bound = serve_with_retry(state, addr, true).await.unwrap();
        release.join().unwrap();
        assert_eq!(bound, addr);
    }

    #[tokio::test]
    async fn connect_receives_connected() {
        let state = AppState::new(Identity {
            device_id: "dev-1".into(),
            name: "TestMac".into(),
        });
        let addr = serve(state, "127.0.0.1:0".parse().unwrap()).await.unwrap();
        let (mut ws, _) = connect_async(format!("ws://{addr}")).await.unwrap();
        let msg = ws.next().await.unwrap().unwrap();
        let Message::Text(text) = msg else {
            panic!("not text")
        };
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["type"], "connected");
        assert_eq!(v["device_id"], "dev-1");
        assert_eq!(v["name"], "TestMac");
        assert_eq!(v["os"], agentpad_input::os());
        assert_eq!(v["sync_enabled"], true);
    }

    #[tokio::test]
    async fn ping_pong_and_paused_ack() {
        let state = AppState::new(Identity {
            device_id: "dev-1".into(),
            name: "TestMac".into(),
        });
        state.set_paused(true);
        let addr = serve(state, "127.0.0.1:0".parse().unwrap()).await.unwrap();
        let (mut ws, _) = connect_async(format!("ws://{addr}")).await.unwrap();
        let _connected = ws.next().await.unwrap().unwrap();
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
}
