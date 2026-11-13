//! Findings from review: stream opens a listener never grants, and the
//! total bandwidth limit applying each way.

use super::{quic::*, *};
use crate::{hub::Access, metrics::Direction};

/// A listener that grants the relay no streams: every incoming dial's
/// stream open would wait forever, as a hostile or broken computer could.
#[tokio::test]
async fn dials_to_a_listener_that_grants_no_streams_leave_nothing_behind() {
    let limits = Limits {
        accept_timeout: Duration::from_millis(300),
        ..Limits::default()
    };
    let relay = start_both(&options(limits)).await;
    let key = signing_key(100);
    let route = RouteId::of(key.verifying_key());
    let mut stingy = crate::quic::client_transport();
    stingy.max_concurrent_bidi_streams(quinn::VarInt::from_u32(0));
    let listener = connect_with(relay.quic, relay.pin, stingy).await.unwrap();
    let (mut send, mut recv) = listener.open_bi().await.unwrap();
    let request = listen_request(&listener, &key, b"");
    send.write_all(&request.encode().unwrap()).await.unwrap();
    assert_eq!(status(&mut recv).await, Status::Ok);
    let keys = crate::wire::Message::DialKeys(vec![public(&phone())]);
    assert_eq!(quic_message(&mut send, &mut recv, keys).await, Status::Ok);

    let phone_connection = connect_to(&relay).await;
    let tasks = || {
        tokio::runtime::Handle::current()
            .metrics()
            .num_alive_tasks()
    };
    let before = tasks();
    for _ in 0..5 {
        let (dialed, _send, _recv) = quic_dial_as(&phone_connection, route, &phone()).await;
        assert_eq!(dialed, Status::Timeout);
    }
    // Each dial's stream-open task ends with its dial, rather than
    // waiting on the listener for good.
    for _ in 0..100 {
        if tasks() <= before {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(tasks() <= before, "{} tasks left, {before} before", tasks());
}

#[tokio::test]
async fn the_total_bandwidth_limit_applies_each_way() {
    let limits = Limits {
        total_bandwidth: 1_000,
        ..Limits::default()
    };
    let hub = Hub::new(limits, Access::Open);
    let up = hub.total_throttle(Direction::FromDialer).unwrap();
    let down = hub.total_throttle(Direction::FromListener).unwrap();
    // A full second's worth each way passes at once: one direction's
    // traffic does not spend the other's allowance.
    assert_eq!(up.reserve(1_000), Duration::ZERO);
    assert_eq!(down.reserve(1_000), Duration::ZERO);
    assert!(up.reserve(500) > Duration::ZERO);
    let unlimited = Hub::new(
        Limits {
            total_bandwidth: 0,
            ..Limits::default()
        },
        Access::Open,
    );
    assert!(unlimited.total_throttle(Direction::FromDialer).is_none());
    // A relay limits total bandwidth unless told not to.
    let default = Hub::new(Limits::default(), Access::Open);
    assert!(default.total_throttle(Direction::FromDialer).is_some());
}
