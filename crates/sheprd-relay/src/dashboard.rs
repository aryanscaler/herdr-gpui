//! The read-only dashboard, behind one access token: connections,
//! sessions, traffic, and refusals, live. It is served only when the
//! operator set a token, and changes nothing on the relay.
//!
//! - `GET /dashboard`: the page, or a redirect to the login.
//! - `GET`/`POST /dashboard/login`, `POST /dashboard/logout`. Three wrong
//!   tokens block the address for 15 minutes (`lockout`).
//! - `GET /dashboard/api/live`: a snapshot a second, as Server-Sent Events.
//! - `GET /dashboard/api/stats`: one snapshot.

pub mod auth;
pub mod history;
pub mod live;
pub mod lockout;

use crate::{
    bucket::{Limiter, Rate},
    guard::BanRow,
    hub::{Hub, SessionRow},
    metrics::{Login, Totals},
    web::Shared,
};
use auth::Account;
use axum::{
    Json, Router,
    body::Bytes,
    extract::{ConnectInfo, DefaultBodyLimit, RawQuery, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response, sse::Sse},
    routing::get,
};
use history::{History, Sample};
use live::Streams;
use lockout::{Lockout, Verdict};
use serde::Serialize;
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

const INDEX: &str = include_str!("dashboard/assets/index.html");
const LOGIN: &str = include_str!("dashboard/assets/login.html");
const SCRIPT: &str = include_str!("dashboard/assets/app.js");
const STYLE: &str = include_str!("dashboard/assets/style.css");
const ICON: &str = include_str!("dashboard/assets/icon.svg");
const FAILED: &str = r#"<p class="error" role="alert">That token is not right. After 3 wrong tries, this address is blocked for 15 minutes.</p>"#;
const BLOCKED: &str = r#"<p class="error" role="alert">Too many wrong tokens from this address. It is blocked for 15 minutes.</p>"#;
const LIMITED: &str = r#"<p class="error" role="alert">Too many sign-in attempts right now. Wait a minute and try again.</p>"#;
const MESSAGE_SLOT: &str = "<!--message-->";
const MAX_FORM: usize = 2 * 1024;
const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; \
img-src 'self'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'";

/// What the dashboard routes share.
#[derive(Clone, Debug)]
struct Board {
    account: Arc<Account>,
    /// Logins from all addresses, so many addresses cannot guess faster.
    everyone: Arc<Mutex<Limiter<()>>>,
    history: Arc<Mutex<History>>,
    live: Arc<Streams>,
    lockout: Arc<Lockout>,
    shared: Shared,
    /// Where `everyone`'s milliseconds count from: a monotonic clock, so a
    /// wall clock stepping back cannot stop its refills.
    started: Instant,
}

/// The dashboard's routes, for an operator `account`.
///
/// Starts sampling throughput, so it must run inside a Tokio runtime.
pub fn router(shared: Shared, account: Account) -> Router {
    let history = Arc::new(Mutex::new(History::default()));
    let hub = Arc::clone(&shared.hub);
    tokio::spawn(history::run(Arc::downgrade(&history), move || {
        let totals = hub.metrics.totals();
        (totals.bytes_from_dialers, totals.bytes_from_listeners)
    }));
    let board = Board {
        account: Arc::new(account),
        everyone: Arc::new(Mutex::new(Limiter::new(Rate {
            burst: 30,
            per_minute: 60,
        }))),
        history,
        live: Arc::default(),
        lockout: Arc::default(),
        shared,
        started: Instant::now(),
    };
    Router::new()
        .route("/dashboard", get(index))
        .route("/dashboard/login", get(login_page).post(login))
        .route("/dashboard/logout", axum::routing::post(logout))
        .route("/dashboard/api/live", get(live_stats))
        .route("/dashboard/api/stats", get(stats))
        .route(
            "/dashboard/app.js",
            get(|| async { asset(SCRIPT, "text/javascript; charset=utf-8") }),
        )
        .route(
            "/dashboard/style.css",
            get(|| async { asset(STYLE, "text/css; charset=utf-8") }),
        )
        .route(
            "/dashboard/icon.svg",
            get(|| async { asset(ICON, "image/svg+xml") }),
        )
        .layer(DefaultBodyLimit::max(MAX_FORM))
        .with_state(board)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// When the request's login expires, in Unix seconds, if it has one.
fn login_expiry(board: &Board, headers: &HeaderMap) -> Option<u64> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(auth::cookie)
        .find_map(|value| board.account.expiry(value, now_secs()))
}

fn signed_in(board: &Board, headers: &HeaderMap) -> bool {
    login_expiry(board, headers).is_some()
}

/// Headers every dashboard response carries.
fn hardened(mut response: Response) -> Response {
    let headers = response.headers_mut();
    let fixed = [
        (header::CACHE_CONTROL, "no-store"),
        (header::CONTENT_SECURITY_POLICY, CSP),
        (header::REFERRER_POLICY, "no-referrer"),
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (header::X_FRAME_OPTIONS, "DENY"),
    ];
    for (name, value) in fixed {
        headers.insert(name, HeaderValue::from_static(value));
    }
    response
}

fn html(body: String) -> Response {
    hardened(([(header::CONTENT_TYPE, "text/html; charset=utf-8")], body).into_response())
}

fn asset(body: &'static str, kind: &'static str) -> Response {
    hardened(([(header::CONTENT_TYPE, kind)], body).into_response())
}

fn redirect(to: &'static str, cookie: Option<String>) -> Response {
    let mut response = (StatusCode::SEE_OTHER, [(header::LOCATION, to)]).into_response();
    if let Some(cookie) = cookie.and_then(|cookie| HeaderValue::from_str(&cookie).ok()) {
        response.headers_mut().insert(header::SET_COOKIE, cookie);
    }
    hardened(response)
}

async fn index(State(board): State<Board>, headers: HeaderMap) -> Response {
    if !signed_in(&board, &headers) {
        return redirect("/dashboard/login", None);
    }
    html(INDEX.to_owned())
}

async fn login_page(RawQuery(query): RawQuery) -> Response {
    let message = match query.as_deref() {
        Some("blocked") => BLOCKED,
        Some("failed") => FAILED,
        Some("limited") => LIMITED,
        _ => "",
    };
    html(LOGIN.replace(MESSAGE_SLOT, message))
}

async fn login(
    State(board): State<Board>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let metrics = &board.shared.hub.metrics;
    // A page on another site cannot post here: otherwise it could submit
    // wrong tokens from a visitor's browser and get the visitor's address
    // locked out and banned. Refused before anything is counted.
    if !crate::web::same_origin(&headers) {
        return hardened(StatusCode::FORBIDDEN.into_response());
    }
    let address = board.shared.address(peer, &headers);
    let now = Instant::now();
    // A blocked address is refused before the limit on all addresses,
    // so its tries do not use that up and lock the operator out.
    if let Verdict::Blocked(_) = board.lockout.check(address, now) {
        metrics.login(Login::Blocked);
        return redirect("/dashboard/login?blocked", None);
    }
    let since_ms = u64::try_from(board.started.elapsed().as_millis()).unwrap_or(u64::MAX);
    if !allow(&board.everyone, (), since_ms) {
        metrics.login(Login::Limited);
        return redirect("/dashboard/login?limited", None);
    }
    // The try is reserved before the token is looked at, under one lock,
    // so a blocked address learns nothing and requests sent at once
    // cannot get more than three guesses between them.
    if let Verdict::Blocked(_) = board.lockout.attempt(address, now) {
        metrics.login(Login::Blocked);
        return redirect("/dashboard/login?blocked", None);
    }
    let accepted = auth::login_form(&body).is_some_and(|token| board.account.check(&token));
    if !accepted {
        metrics.login(Login::Failed);
        if board.lockout.exhausted(address, now) {
            // Blocked from the dashboard only, never banned from the
            // relay: everyone behind a shared address, a carrier's NAT
            // say, would lose their computers for one person's typos.
            tracing::warn!("an address was blocked after wrong dashboard tokens");
            return redirect("/dashboard/login?blocked", None);
        }
        tracing::warn!("a dashboard login failed");
        return redirect("/dashboard/login?failed", None);
    }
    board.lockout.succeed(address);
    metrics.login(Login::Succeeded);
    let Ok(cookie) = board.account.issue(now_secs()) else {
        return hardened(StatusCode::INTERNAL_SERVER_ERROR.into_response());
    };
    redirect("/dashboard", Some(auth::set_cookie(Some(&cookie))))
}

/// Signs this browser's session out on the relay, not only in the browser.
async fn logout(State(board): State<Board>, headers: HeaderMap) -> Response {
    if !crate::web::same_origin(&headers) {
        return hardened(StatusCode::FORBIDDEN.into_response());
    }
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(auth::cookie)
        .for_each(|value| board.account.revoke(value));
    redirect("/dashboard/login", Some(auth::set_cookie(None)))
}

fn allow<K: Eq + std::hash::Hash>(limiter: &Mutex<Limiter<K>>, key: K, now_ms: u64) -> bool {
    match limiter.lock() {
        Ok(mut limiter) => limiter.allow(key, now_ms),
        Err(poisoned) => poisoned.into_inner().allow(key, now_ms),
    }
}

/// The numbers the page draws.
#[derive(Debug, Serialize)]
pub struct Stats {
    /// Bans now, earned and manual.
    pub bans: Vec<BanRow>,
    /// Throughput samples, oldest first.
    pub history: Vec<Sample>,
    pub limits: LimitsView,
    pub sessions: Vec<SessionRow>,
    pub totals: Totals,
    pub version: &'static str,
}

/// The limits the operator set, as the page shows them.
#[derive(Debug, Serialize)]
pub struct LimitsView {
    pub max_connections: usize,
    pub max_connections_per_ip: usize,
    pub max_listeners: usize,
    pub max_sessions: usize,
    pub session_bandwidth: u64,
    pub total_bandwidth: u64,
}

/// The stats for `hub` as the dashboard reads them.
#[must_use]
pub fn snapshot(hub: &Hub, history: Vec<Sample>) -> Stats {
    let limits = hub.limits;
    Stats {
        bans: hub.guard.bans(Instant::now()),
        history,
        limits: LimitsView {
            max_connections: limits.max_connections,
            max_connections_per_ip: limits.max_connections_per_ip,
            max_listeners: limits.max_listeners,
            max_sessions: limits.max_sessions,
            session_bandwidth: limits.session_bandwidth,
            total_bandwidth: limits.total_bandwidth,
        },
        sessions: hub.sessions(),
        totals: hub.metrics.totals(),
        version: env!("CARGO_PKG_VERSION"),
    }
}

fn current(board: &Board) -> Stats {
    let history = match board.history.lock() {
        Ok(history) => history.samples(),
        Err(poisoned) => poisoned.into_inner().samples(),
    };
    snapshot(&board.shared.hub, history)
}

async fn stats(State(board): State<Board>, headers: HeaderMap) -> Response {
    if !signed_in(&board, &headers) {
        return hardened(StatusCode::UNAUTHORIZED.into_response());
    }
    hardened(Json(current(&board)).into_response())
}

/// A snapshot a second until the login expires.
async fn live_stats(State(board): State<Board>, headers: HeaderMap) -> Response {
    let Some(expires) = login_expiry(&board, &headers) else {
        return hardened(StatusCode::UNAUTHORIZED.into_response());
    };
    let Some(place) = board.live.open() else {
        return hardened(StatusCode::SERVICE_UNAVAILABLE.into_response());
    };
    let (events, keep_alive) = live::events(place, expires, move || {
        serde_json::to_string(&current(&board)).unwrap_or_default()
    });
    hardened(Sse::new(events).keep_alive(keep_alive).into_response())
}
