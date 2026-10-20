//! The socket every provider shares: open the WebSocket with the ephemeral
//! token as `Authorization: Bearer`, then relay JSON text frames through a
//! provider's codec until either side closes. The token is never logged, and
//! a failed connect names what failed, not the address (it can carry the
//! token).

use futures::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;

use super::{ClientEvent, Connection, RealtimeError, ServerEvent};

/// A provider's wire format: one app event as zero or more frames, and one
/// frame as a normalized event (`None` for frames the call doesn't use).
pub(crate) struct Codec {
    pub encode: fn(&ClientEvent) -> Vec<Value>,
    pub decode: fn(&str) -> Option<ServerEvent>,
}

pub(crate) async fn open(
    ws_url: &str,
    token: &str,
    codec: Codec,
) -> Result<Connection, RealtimeError> {
    let mut req = ws_url
        .into_client_request()
        .map_err(|e| RealtimeError::Connect(e.to_string()))?;
    let bearer = HeaderValue::from_str(&format!("Bearer {token}"))
        .map_err(|_| RealtimeError::Connect("the token isn't a header value".into()))?;
    req.headers_mut().insert("Authorization", bearer);
    let (socket, _) = match tokio_tungstenite::connect_async(req).await {
        Ok(ok) => ok,
        Err(tokio_tungstenite::tungstenite::Error::Http(r)) if r.status().as_u16() == 401 => {
            return Err(RealtimeError::Refused)
        }
        // The URL carries the token: say what failed, not where.
        Err(e) => return Err(RealtimeError::Connect(kind_of(&e))),
    };
    let (mut sink, mut stream) = socket.split();
    let (to_server, mut outbox) = mpsc::channel::<ClientEvent>(256);
    let (inbox, from_server) = mpsc::channel::<ServerEvent>(256);
    tokio::spawn(async move {
        'relay: loop {
            tokio::select! {
                out = outbox.recv() => {
                    let Some(event) = out else {
                        let _ = sink.close().await;
                        break;
                    };
                    for frame in (codec.encode)(&event) {
                        if sink.send(Message::text(frame.to_string())).await.is_err() {
                            let _ = inbox.send(ServerEvent::Closed).await;
                            break 'relay;
                        }
                    }
                }
                frame = stream.next() => {
                    match frame {
                        Some(Ok(Message::Text(text))) => {
                            if let Some(event) = (codec.decode)(text.as_str()) {
                                if inbox.send(event).await.is_err() {
                                    break;
                                }
                            }
                        }
                        Some(Ok(Message::Close(_))) | None | Some(Err(_)) => {
                            let _ = inbox.send(ServerEvent::Closed).await;
                            break;
                        }
                        Some(Ok(_)) => {}
                    }
                }
            }
        }
    });
    Ok(Connection {
        tx: to_server,
        rx: from_server,
    })
}

pub(crate) fn kind_of(e: &tokio_tungstenite::tungstenite::Error) -> String {
    use tokio_tungstenite::tungstenite::Error as E;
    match e {
        E::Io(io) => format!("{:?}", io.kind()),
        E::Http(r) => format!("HTTP {}", r.status().as_u16()),
        E::Url(_) => "a bad address".into(),
        E::Tls(_) => "TLS".into(),
        other => format!("{other}")
            .split(':')
            .next()
            .unwrap_or("error")
            .to_string(),
    }
}
