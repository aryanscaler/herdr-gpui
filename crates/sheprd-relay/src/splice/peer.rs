//! One side of a session, over either transport: a QUIC stream, or a
//! WebSocket for networks without UDP. A session may mix them.

use crate::{metrics::End, wire};
use axum::{
    body::Bytes,
    extract::ws::{CloseFrame, Message, WebSocket},
};
use futures_util::{
    SinkExt, StreamExt,
    stream::{SplitSink, SplitStream},
};
use quinn::{RecvStream, SendStream, VarInt};

/// A dialer or listener, ready to be spliced.
#[derive(Debug)]
pub enum Peer {
    /// Boxed: a WebSocket is several times a stream pair's size, and only
    /// the fallback uses it.
    Socket(Box<WebSocket>),
    Stream {
        recv: RecvStream,
        send: SendStream,
    },
}

impl Peer {
    /// The halves bytes come from and go to.
    pub(crate) fn split(self) -> (Source, Sink) {
        match self {
            Self::Socket(socket) => {
                let (sink, source) = (*socket).split();
                (Source::Socket(source), Sink::Socket(sink))
            }
            Self::Stream { recv, send } => (Source::Stream(recv), Sink::Stream(send)),
        }
    }
}

/// What a source gave.
pub(crate) enum Next {
    Data(Bytes),
    /// The peer finished or went away.
    End,
    /// The peer sent something the protocol forbids: a text frame.
    Violation,
}

pub(crate) enum Source {
    Socket(SplitStream<WebSocket>),
    Stream(RecvStream),
}

impl Source {
    /// The next bytes, at most `max` of them from a stream; a WebSocket's
    /// frames are already bounded by its upgrade.
    pub(crate) async fn next(&mut self, max: usize) -> Next {
        match self {
            Self::Socket(socket) => loop {
                return match socket.next().await {
                    Some(Ok(Message::Binary(bytes))) => Next::Data(bytes),
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
                    Some(Ok(Message::Text(_))) => Next::Violation,
                    Some(Ok(Message::Close(_)) | Err(_)) | None => Next::End,
                };
            },
            Self::Stream(recv) => match recv.read_chunk(max, true).await {
                Ok(Some(chunk)) => Next::Data(chunk.bytes),
                Ok(None) | Err(_) => Next::End,
            },
        }
    }
}

pub(crate) enum Sink {
    Socket(SplitSink<WebSocket, Message>),
    Stream(SendStream),
}

impl Sink {
    /// Sends `bytes`, or says the peer is gone.
    pub(crate) async fn send(&mut self, bytes: Bytes) -> bool {
        match self {
            Self::Socket(socket) => socket.send(Message::Binary(bytes)).await.is_ok(),
            Self::Stream(send) => send.write_chunk(bytes).await.is_ok(),
        }
    }

    /// Ends this side for `end`: a close frame on a WebSocket, a finish or
    /// reset on a stream.
    pub(crate) async fn finish(&mut self, end: End) {
        match self {
            Self::Socket(socket) => {
                let frame = CloseFrame {
                    code: socket_code(end),
                    reason: "".into(),
                };
                let _ = socket.send(Message::Close(Some(frame))).await;
            }
            Self::Stream(send) => {
                let _ = match end {
                    End::Closed => send.finish(),
                    _ => send.reset(VarInt::from_u32(stream_code(end))),
                };
            }
        }
    }
}

fn socket_code(end: End) -> u16 {
    use super::code;
    match end {
        End::Closed => code::PEER_GONE,
        End::Expired => code::EXPIRED,
        End::Idle => code::IDLE,
        End::Protocol => code::PROTOCOL,
        End::Revoked => code::REVOKED,
        End::Banned => code::BANNED,
    }
}

fn stream_code(end: End) -> u32 {
    match end {
        End::Closed => wire::code::PEER_GONE,
        End::Expired => wire::code::EXPIRED,
        End::Idle => wire::code::IDLE,
        End::Protocol => wire::code::PROTOCOL,
        End::Revoked => wire::code::REVOKED,
        End::Banned => wire::code::BANNED,
    }
}
