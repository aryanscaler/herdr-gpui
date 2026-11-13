//! Starting the relay: QUIC, its HTTP routes and dashboard, and its
//! metrics port.

pub(crate) mod capped;

use crate::{
    Error, Options, Result,
    dashboard::{
        self,
        auth::Account,
        lockout::{Lockout, Verdict},
    },
    endpoint::QuicInfo,
    guard::{Cidr, parse_list},
    hub::{Access, Hub},
    quic::{self, tls::Identity},
    secret::Secret,
    web::{self, Shared, Source},
};
use axum::{
    Router,
    extract::{ConnectInfo, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
    serve::ListenerExt,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use capped::Capped;
use secrecy::ExposeSecret;
use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
};
use tokio::net::TcpListener;

/// HTTP connections beyond the hub's sockets: plain requests such as
/// `/v1/info` and the dashboard, in all and from one caller.
const HTTP_HEADROOM: usize = 1_024;
const HTTP_HEADROOM_PER_CALLER: usize = 16;

/// The shortest listen or metrics token the relay accepts.
const MIN_TOKEN: usize = 16;

/// The relay's HTTP routes and, with a token, the dashboard. `quic` is
/// what `/v1/info` says about QUIC.
pub fn app(options: &Options, quic: Option<QuicInfo>) -> Result<(Router, Arc<Hub>)> {
    let hub = Arc::new(Hub::new(options.limits, access(options)?));
    let shared = Shared {
        hub: Arc::clone(&hub),
        public_host: options.public_host.as_deref().map(Arc::from),
        quic,
        source: source(options),
    };
    let mut router = web::router(shared.clone());
    if let Some(token) = &options.dashboard_token {
        let account = Account::new(token.expose_secret())?;
        router = router.merge(dashboard::router(shared.clone(), account));
    }
    hub.guard.set_manual(options.bans.clone());
    // Bans apply to every route, the dashboard included.
    let router = router.layer(axum::middleware::from_fn_with_state(
        shared,
        web::refuse_banned,
    ));
    Ok((router, hub))
}

/// Where callers' addresses come from, for per-address limits.
fn source(options: &Options) -> Source {
    let header = options
        .client_ip_header
        .as_deref()
        .and_then(|name| axum::http::HeaderName::from_bytes(name.as_bytes()).ok());
    match (header, options.behind_proxy) {
        (Some(header), _) => Source::Header(header),
        (None, true) => Source::Proxy,
        (None, false) => Source::Peer,
    }
}

/// Who may listen: anyone, unless the operator set listen tokens.
fn access(options: &Options) -> Result<Access> {
    let Some(tokens) = &options.listen_token else {
        return Ok(Access::Open);
    };
    Access::tokens(tokens.expose_secret())
}

/// What the metrics port's routes share.
#[derive(Clone, Debug)]
struct MetricsState {
    hub: Arc<Hub>,
    /// Wrong tokens per caller, as the dashboard counts them: three block
    /// the caller for 15 minutes, unchecked, so the token cannot be
    /// guessed at the port's speed.
    lockout: Arc<Lockout>,
    token: Option<Arc<Secret>>,
}

/// The metrics port's routes.
pub fn metrics_app(hub: Arc<Hub>, token: Option<Secret>) -> Router {
    Router::new()
        .route("/metrics", get(metrics))
        .with_state(MetricsState {
            hub,
            lockout: Arc::default(),
            token: token.map(Arc::new),
        })
}

async fn metrics(State(state): State<MetricsState>, request: axum::extract::Request) -> Response {
    let MetricsState {
        hub,
        lockout,
        token,
    } = state;
    if let Some(token) = token {
        // Served with the caller's address; a router used without one
        // counts every caller as one.
        let caller = request
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map_or(IpAddr::from([0, 0, 0, 0]), |info| info.0.ip());
        let now = std::time::Instant::now();
        if let Verdict::Blocked(_) = lockout.attempt(caller, now) {
            return StatusCode::TOO_MANY_REQUESTS.into_response();
        }
        let given = request
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .unwrap_or_default();
        // `attempt` counted this try as wrong; a right one clears it.
        if !token.matches(given.as_bytes()) {
            return StatusCode::UNAUTHORIZED.into_response();
        }
        lockout.succeed(caller);
    }
    (
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        hub.metrics.render() + &banned_now(&hub),
    )
        .into_response()
}

/// The gauge of addresses banned now, which only the guard knows.
fn banned_now(hub: &Hub) -> String {
    let active = hub.guard.active(std::time::Instant::now());
    format!(
        "# HELP sheprd_relay_banned Addresses banned automatically now.\n\
         # TYPE sheprd_relay_banned gauge\nsheprd_relay_banned {active}\n"
    )
}

/// Keeps the guard's manual ranges in step with `file` (and `--ban`),
/// re-reading it every [`BAN_FILE_EVERY`] and ending what newly falls in
/// them. A missing or unreadable file keeps the last good list.
pub(crate) async fn watch_ban_file(hub: Arc<Hub>, file: std::path::PathBuf, fixed: Vec<Cidr>) {
    let mut ticks = tokio::time::interval(BAN_FILE_EVERY);
    let mut last: Option<String> = None;
    // Warned once each time reading starts failing, not every 30 s.
    let mut failing = false;
    loop {
        ticks.tick().await;
        let path = file.clone();
        let read = tokio::task::spawn_blocking(move || std::fs::read_to_string(path)).await;
        let text = match read {
            Ok(Ok(text)) => text,
            Ok(Err(error)) => {
                if !failing {
                    tracing::warn!(%error, "could not read the ban file; keeping its last bans");
                    failing = true;
                }
                continue;
            }
            Err(_) => continue,
        };
        failing = false;
        if last.as_ref() == Some(&text) {
            continue;
        }
        let (ranges, bad) = parse_list(&text);
        if !bad.is_empty() {
            tracing::warn!(entries = ?bad, "some ban file entries are not addresses or ranges");
        }
        tracing::info!(ranges = ranges.len(), "loaded the ban file");
        let all: Vec<Cidr> = fixed.iter().copied().chain(ranges).collect();
        hub.guard.set_manual(all.clone());
        hub.sweep();
        last = Some(text);
    }
}

/// Re-reads the listen token file every [`BAN_FILE_EVERY`], so tokens can
/// be added and revoked without a restart ([`reload_token_file`]).
async fn watch_token_file(hub: Arc<Hub>, file: std::path::PathBuf) {
    let mut ticks = tokio::time::interval(BAN_FILE_EVERY);
    let mut last: Option<String> = None;
    loop {
        ticks.tick().await;
        let (hub, file) = (Arc::clone(&hub), file.clone());
        // Reading the file is blocking I/O; so is the hub's lock, briefly.
        let reloaded = tokio::task::spawn_blocking(move || {
            reload_token_file(&hub, &file, &mut last);
            last
        })
        .await;
        let Ok(reloaded) = reloaded else {
            return;
        };
        last = reloaded;
    }
}

/// One look at the listen token file: when it changed since `last`, its
/// tokens replace the hub's and a revoked token's listeners close. An
/// empty or deleted file revokes them all; one that does not parse, or
/// cannot be read for another reason, keeps the tokens already loaded.
pub(crate) fn reload_token_file(hub: &Hub, file: &std::path::Path, last: &mut Option<String>) {
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            tracing::warn!(%error, "could not read the listen token file; keeping its last tokens");
            return;
        }
    };
    if last.as_ref() == Some(&text) {
        return;
    }
    match hub.access.replace(&text) {
        Ok(kept) => {
            tracing::info!(tokens = kept.len(), "loaded the listen token file");
            hub.keep_tokens(&kept);
        }
        Err(error) => {
            tracing::warn!(%error, "the listen token file is not usable; keeping its last tokens");
        }
    }
    *last = Some(text);
}

/// How often the ban file is re-read.
const BAN_FILE_EVERY: std::time::Duration = std::time::Duration::from_secs(30);

fn strong(token: &str, name: &'static str) -> Result<()> {
    if token.chars().count() < MIN_TOKEN {
        return Err(Error::WeakSecret {
            min: MIN_TOKEN,
            name,
        });
    }
    Ok(())
}

async fn bind(address: SocketAddr) -> Result<TcpListener> {
    TcpListener::bind(address)
        .await
        .map_err(|source| Error::Bind { address, source })
}

/// Runs the relay until it is told to stop.
pub async fn serve(options: Options) -> Result<()> {
    let quic = options
        .quic
        .map(|address| bind_quic(address, options.quic_port, options.quic_identity.as_deref()))
        .transpose()?;
    let (router, hub) = app(&options, quic.as_ref().map(|(_, info)| info.clone()))?;
    if let Some(file) = options.ban_file.clone() {
        tokio::spawn(watch_ban_file(Arc::clone(&hub), file, options.bans.clone()));
    }
    if let Some(file) = options.listen_token_file.clone() {
        tokio::spawn(watch_token_file(Arc::clone(&hub), file));
    }
    if !options.limits.auto_ban {
        tracing::info!("automatic bans are off");
    }
    if let Some((endpoint, info)) = &quic {
        tokio::spawn(quic::run(endpoint.clone(), Arc::clone(&hub)));
        tracing::info!(port = info.port, pin = %info.pin, "serving QUIC");
    } else {
        tracing::info!("QUIC is off: clients use the WebSocket fallback");
    }
    let metrics_token = options
        .metrics_token
        .as_ref()
        .map(ExposeSecret::expose_secret)
        .map(|token| {
            strong(token, "the metrics token").and_then(|()| Secret::new(token.as_bytes()))
        })
        .transpose()?;
    let listener = bind(options.listen).await?;
    if let Some(address) = options.metrics {
        let metrics = bind(address).await?;
        let app = metrics_app(Arc::clone(&hub), metrics_token);
        tokio::spawn(async move {
            let app = app.into_make_service_with_connect_info::<SocketAddr>();
            if let Err(error) = axum::serve(metrics, app)
                .with_graceful_shutdown(stopped())
                .await
            {
                tracing::error!(%error, "the metrics server stopped");
            }
        });
        tracing::info!(%address, "serving /metrics");
    }
    if options.public_host.is_none() {
        tracing::warn!("the WebSocket fallback is off: set RELAY_PUBLIC_HOST to the relay's name");
    }
    if options.dashboard_token.is_some() {
        tracing::info!("the dashboard is at /dashboard");
    } else {
        tracing::info!("the dashboard is off: set RELAY_DASHBOARD_TOKEN to turn it on");
    }
    tracing::info!(
        address = %options.listen,
        limits = ?options.limits,
        "the relay is listening"
    );
    // Room for every socket the hub admits, and plain requests beside.
    let total = options.limits.max_connections.saturating_add(HTTP_HEADROOM);
    // Behind a proxy every connection is the proxy's, so only the total
    // applies here; the hub limits callers by the address it reports.
    let per_caller = (source(&options) == Source::Peer).then(|| {
        options
            .limits
            .max_connections_per_ip
            .saturating_add(HTTP_HEADROOM_PER_CALLER)
    });
    let idle = options.limits.idle_timeout;
    let listener = Capped::new(listener, total, per_caller, idle).tap_io(|_| ());
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(stopped())
    .await
    .map_err(Error::Serve)?;
    if let Some((endpoint, _)) = quic {
        endpoint.close(quinn::VarInt::from_u32(crate::wire::code::SHUTDOWN), b"");
        endpoint.wait_idle().await;
    }
    Ok(())
}

/// Binds QUIC on `address` with the identity kept in `identity`, or a
/// fresh one; what clients learn of it.
pub fn bind_quic(
    address: SocketAddr,
    port: Option<u16>,
    identity: Option<&std::path::Path>,
) -> Result<(quinn::Endpoint, QuicInfo)> {
    let identity = match identity {
        Some(directory) => Identity::kept(directory)?,
        None => Identity::generate()?,
    };
    let endpoint = quic::bind(address, &identity)?;
    let bound = endpoint
        .local_addr()
        .map_err(|source| quic::Error::Bind { address, source })?;
    let info = QuicInfo {
        pin: URL_SAFE_NO_PAD.encode(identity.pin),
        port: port.unwrap_or(bound.port()),
    };
    Ok((endpoint, info))
}

/// Resolves on Ctrl-C or, where there is one, `SIGTERM`, which is what
/// `docker stop` sends.
async fn stopped() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        if let Ok(mut terminate) = signal(SignalKind::terminate()) {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = terminate.recv() => {}
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}
