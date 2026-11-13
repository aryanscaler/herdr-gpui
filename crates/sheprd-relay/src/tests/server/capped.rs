//! The HTTP listener's own bounds: connections in all, and one that sends
//! nothing.

use crate::serve::capped::Capped;
use axum::serve::Listener;
use std::time::Duration;
use tokio::{
    io::AsyncReadExt,
    net::{TcpListener, TcpStream},
    time::timeout,
};

async fn capped(total: usize) -> Capped {
    let inner = TcpListener::bind("127.0.0.1:0").await.unwrap();
    Capped::new(inner, total, Some(4), Duration::from_secs(60))
}

#[tokio::test]
async fn a_full_listener_waits_for_room_before_accepting() {
    let mut listener = capped(1).await;
    let address = listener.local_addr().unwrap();
    let _first = TcpStream::connect(address).await.unwrap();
    let (held, _) = listener.accept().await;
    let _second = TcpStream::connect(address).await.unwrap();
    assert!(
        timeout(Duration::from_millis(200), listener.accept())
            .await
            .is_err(),
        "a second connection was accepted past the cap"
    );
    drop(held);
    timeout(Duration::from_secs(5), listener.accept())
        .await
        .expect("room given back was not used");
}

#[tokio::test(start_paused = true)]
async fn a_connection_that_sends_nothing_is_dropped() {
    let mut listener = capped(4).await;
    let address = listener.local_addr().unwrap();
    let _silent = TcpStream::connect(address).await.unwrap();
    let (mut held, _) = listener.accept().await;
    let mut byte = [0; 1];
    let error = held.read(&mut byte).await.unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
}
