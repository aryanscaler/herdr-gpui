//! The dashboard over HTTP: the login, the cookie it sets, and the stats
//! and metrics behind it.

use super::*;
use crate::secret::Secret;
use crate::serve::metrics_app;
use axum::{
    body::Body,
    extract::connect_info::MockConnectInfo,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use tower::ServiceExt;

fn router() -> axum::Router {
    // Behind a proxy, so each test request can name its caller's address.
    let options = Options {
        behind_proxy: true,
        ..options(Limits::default())
    };
    let (router, _hub) = app(&options, None).unwrap();
    router.layer(MockConnectInfo(SocketAddr::from(([10, 0, 0, 1], 1))))
}

const HOME: &str = "198.51.100.7";

/// Signs in from `from` with `token`.
async fn login_from(router: &axum::Router, from: &str, token: &str) -> axum::response::Response {
    let body = format!("token={}", token.replace(' ', "+"));
    let request = Request::post("/dashboard/login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header("x-forwarded-for", from)
        .body(Body::from(body))
        .unwrap();
    router.clone().oneshot(request).await.unwrap()
}

fn location(response: &axum::response::Response) -> &str {
    response.headers()[header::LOCATION].to_str().unwrap()
}

fn cookie_pair(response: &axum::response::Response) -> String {
    let set = response.headers()[header::SET_COOKIE].to_str().unwrap();
    set.split(';').next().unwrap().to_owned()
}

async fn get(router: &axum::Router, path: &str, cookie: Option<&str>) -> axum::response::Response {
    let mut request = Request::get(path);
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    router
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn the_page_stats_and_live_stream_need_a_login() {
    let router = router();
    let page = get(&router, "/dashboard", None).await;
    assert_eq!(page.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&page), "/dashboard/login");
    for path in ["/dashboard/api/stats", "/dashboard/api/live"] {
        assert_eq!(
            get(&router, path, None).await.status(),
            StatusCode::UNAUTHORIZED,
            "{path}"
        );
        let forged = get(&router, path, Some("sheprd_relay=99999999999.AAAA")).await;
        assert_eq!(forged.status(), StatusCode::UNAUTHORIZED, "{path}");
    }
}

#[tokio::test]
async fn the_token_sets_a_cookie_that_opens_the_stats() {
    let router = router();
    let response = login_from(&router, HOME, TOKEN).await;
    assert_eq!(location(&response), "/dashboard");
    let pair = cookie_pair(&response);
    assert!(
        response.headers()[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap()
            .contains("frame-ancestors 'none'")
    );
    let stats = get(&router, "/dashboard/api/stats", Some(&pair)).await;
    assert_eq!(stats.status(), StatusCode::OK);
    let body = stats.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["totals"]["sessions"], 0);
    assert_eq!(json["limits"]["max_connections"], 10_000);
}

#[tokio::test]
async fn three_wrong_tokens_block_the_address_even_for_the_right_one() {
    let router = router();
    let attacker = "203.0.113.9";
    for attempt in 1..=2 {
        let response = login_from(&router, attacker, "a wrong token of enough length").await;
        assert_eq!(
            location(&response),
            "/dashboard/login?failed",
            "try {attempt}"
        );
        assert!(response.headers().get(header::SET_COOKIE).is_none());
    }
    let third = login_from(&router, attacker, "a wrong token of enough length").await;
    assert_eq!(location(&third), "/dashboard/login?blocked");
    // The right token is now refused, unchecked, for 15 minutes. The
    // address is blocked from the dashboard only, not banned from the
    // relay (`bans`).
    let right = login_from(&router, attacker, TOKEN).await;
    assert_eq!(location(&right), "/dashboard/login?blocked");
    assert!(right.headers().get(header::SET_COOKIE).is_none());
    // Its IPv6 neighbours in the same /64 are the same guesser.
    for from in ["2001:db8::1", "2001:db8::2", "2001:db8::3"] {
        login_from(&router, from, "wrong").await;
    }
    let neighbour = login_from(&router, "2001:db8::ffff", TOKEN).await;
    assert_eq!(location(&neighbour), "/dashboard/login?blocked");
    // So is a private address.
    let inside = "10.1.2.3";
    for _ in 0..3 {
        login_from(&router, inside, "wrong").await;
    }
    let right = login_from(&router, inside, TOKEN).await;
    assert_eq!(location(&right), "/dashboard/login?blocked");
    // Everyone else still signs in.
    assert_eq!(
        location(&login_from(&router, HOME, TOKEN).await),
        "/dashboard"
    );
}

#[tokio::test]
async fn signing_in_forgets_earlier_wrong_tries() {
    let router = router();
    for _ in 0..2 {
        login_from(&router, HOME, "wrong").await;
    }
    assert_eq!(
        location(&login_from(&router, HOME, TOKEN).await),
        "/dashboard"
    );
    for _ in 0..2 {
        let response = login_from(&router, HOME, "wrong").await;
        assert_eq!(location(&response), "/dashboard/login?failed");
    }
}

#[tokio::test]
async fn the_login_page_explains_each_refusal() {
    let router = router();
    for (query, text) in [
        ("failed", "After 3 wrong tries"),
        ("blocked", "blocked for 15 minutes"),
        ("limited", "Too many sign-in attempts"),
    ] {
        let page = get(&router, &format!("/dashboard/login?{query}"), None).await;
        let body = page.into_body().collect().await.unwrap().to_bytes();
        assert!(
            std::str::from_utf8(&body).unwrap().contains(text),
            "{query}"
        );
    }
}

#[tokio::test]
async fn the_live_stream_sends_snapshots() {
    let router = router();
    let pair = cookie_pair(&login_from(&router, HOME, TOKEN).await);
    let live = get(&router, "/dashboard/api/live", Some(&pair)).await;
    assert_eq!(live.status(), StatusCode::OK);
    assert_eq!(live.headers()[header::CONTENT_TYPE], "text/event-stream");
    let mut body = live.into_body();
    let frame = tokio::time::timeout(Duration::from_secs(5), body.frame())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let text = String::from_utf8(frame.into_data().unwrap().to_vec()).unwrap();
    assert!(text.starts_with("event: stats\ndata: {"), "{text}");
    assert!(text.contains("\"totals\""), "{text}");
}

#[tokio::test]
async fn live_streams_are_capped() {
    let router = router();
    let pair = cookie_pair(&login_from(&router, HOME, TOKEN).await);
    let mut open = Vec::new();
    for _ in 0..crate::dashboard::live::MAX_STREAMS {
        let live = get(&router, "/dashboard/api/live", Some(&pair)).await;
        assert_eq!(live.status(), StatusCode::OK);
        open.push(live);
    }
    let one_more = get(&router, "/dashboard/api/live", Some(&pair)).await;
    assert_eq!(one_more.status(), StatusCode::SERVICE_UNAVAILABLE);
    // Closing one frees its place.
    open.pop();
    let again = get(&router, "/dashboard/api/live", Some(&pair)).await;
    assert_eq!(again.status(), StatusCode::OK);
}

#[tokio::test]
async fn the_dashboard_is_absent_without_a_token() {
    let (router, _hub) = app(&Options::default(), None).unwrap();
    let response = router
        .oneshot(Request::get("/dashboard").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn metrics_honour_their_token() {
    let hub = Arc::new(Hub::new(Limits::default(), crate::hub::Access::Open));
    let open = metrics_app(Arc::clone(&hub), None)
        .oneshot(Request::get("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(open.status(), StatusCode::OK);

    let token = Secret::new(b"a metrics token!").unwrap();
    let guarded = metrics_app(Arc::clone(&hub), Some(token));
    let refused = guarded
        .clone()
        .oneshot(Request::get("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(refused.status(), StatusCode::UNAUTHORIZED);
    let allowed = guarded
        .oneshot(
            Request::get("/metrics")
                .header(header::AUTHORIZATION, "Bearer a metrics token!")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(allowed.status(), StatusCode::OK);
    let body = allowed.into_body().collect().await.unwrap().to_bytes();
    assert!(
        std::str::from_utf8(&body)
            .unwrap()
            .contains("sheprd_relay_sessions 0")
    );
}

#[test]
fn a_client_ip_header_names_the_caller() {
    use crate::web::{Shared, Source};
    let shared = |source| Shared {
        hub: Arc::new(Hub::new(Limits::default(), crate::hub::Access::Open)),
        public_host: None,
        quic: None,
        source,
    };
    let peer = SocketAddr::from(([10, 0, 0, 1], 1));
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("do-connecting-ip", "203.0.113.9".parse().unwrap());
    headers.insert("x-forwarded-for", "198.51.100.1, 10.0.0.1".parse().unwrap());
    let header = Source::Header(axum::http::HeaderName::from_static("do-connecting-ip"));
    assert_eq!(
        shared(header).address(peer, &headers).to_string(),
        "203.0.113.9"
    );
    assert_eq!(
        shared(Source::Proxy).address(peer, &headers).to_string(),
        "10.0.0.1"
    );
    assert_eq!(shared(Source::Peer).address(peer, &headers), peer.ip());
    // A missing or garbled header falls back to the peer.
    let missing = Source::Header(axum::http::HeaderName::from_static("cf-connecting-ip"));
    assert_eq!(shared(missing).address(peer, &headers), peer.ip());
}

#[tokio::test]
async fn a_cross_site_login_is_refused_and_not_counted() {
    let router = router();
    let attacker_page = |token: &str| {
        Request::post("/dashboard/login")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header("x-forwarded-for", HOME)
            .header("host", "relay.example.org")
            .header("origin", "https://evil.example")
            .body(Body::from(format!("token={token}")))
            .unwrap()
    };
    for _ in 0..5 {
        let response = router
            .clone()
            .oneshot(attacker_page("wrong"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    // None of those counted: the visitor still signs in.
    assert_eq!(
        location(&login_from(&router, HOME, TOKEN).await),
        "/dashboard"
    );
}

#[tokio::test]
async fn a_blocked_guesser_cannot_use_up_the_operators_logins() {
    let router = router();
    let attacker = "203.0.113.10";
    for _ in 0..3 {
        login_from(&router, attacker, "a wrong token of enough length").await;
    }
    // Far more tries than the limit on all addresses allows at once.
    for _ in 0..100 {
        let response = login_from(&router, attacker, "a wrong token of enough length").await;
        assert_eq!(location(&response), "/dashboard/login?blocked");
    }
    assert_eq!(
        location(&login_from(&router, HOME, TOKEN).await),
        "/dashboard"
    );
}

#[tokio::test]
async fn the_metrics_token_cannot_be_guessed_at_the_ports_speed() {
    let hub = Arc::new(Hub::new(Limits::default(), crate::hub::Access::Open));
    let token = Secret::new(b"a metrics token!").unwrap();
    let guarded = metrics_app(hub, Some(token));
    let scrape = |bearer: &'static str| {
        let guarded = guarded.clone();
        async move {
            let request = Request::get("/metrics")
                .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
                .body(Body::empty())
                .unwrap();
            guarded.oneshot(request).await.unwrap().status()
        }
    };
    for _ in 0..3 {
        assert_eq!(scrape("a wrong guess").await, StatusCode::UNAUTHORIZED);
    }
    // Blocked now, even for the right token, unchecked.
    assert_eq!(
        scrape("a metrics token!").await,
        StatusCode::TOO_MANY_REQUESTS
    );
}
