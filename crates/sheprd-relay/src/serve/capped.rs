//! The HTTP listener, bounded: how many connections it holds in all and
//! from each caller and its wide block, and how long one may sit with
//! nothing crossing it. The hub counts WebSockets, but a plain request or
//! an idle TCP connection was bounded only by the proxy in front, if any.

use crate::{guard::exempt, hub::tally::Tally};
use axum::serve::Listener;
use std::{
    io,
    net::{IpAddr, SocketAddr},
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::{TcpListener, TcpStream},
    sync::{OwnedSemaphorePermit, Semaphore},
    time::{Instant, Sleep, sleep},
};

/// How much longer than a session's own idle timeout a connection may
/// carry nothing either way. Listeners are pinged every 25 seconds and
/// sessions end at their idle timeout, so only a connection doing nothing
/// at all meets this.
const IDLE_MARGIN: Duration = Duration::from_secs(60);
/// How long a caller has, from being accepted, before the relay answers
/// it: its first request must arrive whole by then, however slowly it
/// trickles in.
const QUIET: Duration = Duration::from_secs(30);

/// Connections held by each caller and block, as the limits key them.
type Callers = Arc<Mutex<Tally>>;

/// A TCP listener that waits for room before accepting.
#[derive(Debug)]
pub struct Capped {
    callers: Callers,
    idle: Duration,
    inner: TcpListener,
    per_caller: Option<usize>,
    room: Arc<Semaphore>,
}

impl Capped {
    /// At most `total` connections, and `per_caller` from one public
    /// address (a wide block, [`crate::limits::WIDE_SHARE`] times that);
    /// each closed once nothing crossed it for a minute past `idle`, a
    /// session's idle timeout. `per_caller` is `None` behind a proxy,
    /// where every connection comes from the proxy, whatever its address:
    /// the hub then limits callers by the address the proxy reports.
    #[must_use]
    pub fn new(
        inner: TcpListener,
        total: usize,
        per_caller: Option<usize>,
        idle: Duration,
    ) -> Self {
        Self {
            callers: Arc::default(),
            idle: idle.saturating_add(IDLE_MARGIN),
            inner,
            per_caller,
            room: Arc::new(Semaphore::new(total.max(1))),
        }
    }
}

impl Listener for Capped {
    type Io = Held;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            // Waiting here leaves callers in the kernel's backlog, rather
            // than accepting and dropping them.
            let Ok(permit) = Arc::clone(&self.room).acquire_owned().await else {
                std::future::pending::<()>().await;
                continue;
            };
            let (stream, peer) = match self.inner.accept().await {
                Ok(accepted) => accepted,
                Err(error) => {
                    // As axum's own listener does: a connection that broke
                    // before it was accepted is no reason to wait.
                    if !matches!(
                        error.kind(),
                        io::ErrorKind::ConnectionRefused
                            | io::ErrorKind::ConnectionAborted
                            | io::ErrorKind::ConnectionReset
                    ) {
                        sleep(Duration::from_secs(1)).await;
                    }
                    continue;
                }
            };
            let Some(caller) = Caller::claim(&self.callers, peer.ip(), self.per_caller) else {
                continue;
            };
            return (Held::new(stream, permit, caller, self.idle), peer);
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        self.inner.local_addr()
    }
}

/// One caller's count, given back when its connection drops.
#[derive(Debug)]
struct Caller {
    callers: Callers,
    /// The address counted, unless none was.
    counted: Option<IpAddr>,
}

impl Caller {
    fn claim(callers: &Callers, address: IpAddr, limit: Option<usize>) -> Option<Self> {
        let uncounted = || {
            Some(Self {
                callers: Arc::clone(callers),
                counted: None,
            })
        };
        // A private address is the proxy in front, which carries everyone.
        let Some(limit) = limit.filter(|_| !exempt(address)) else {
            return uncounted();
        };
        lock(callers).try_add(address, limit).then(|| Self {
            callers: Arc::clone(callers),
            counted: Some(address),
        })
    }
}

impl Drop for Caller {
    fn drop(&mut self) {
        if let Some(address) = self.counted {
            lock(&self.callers).remove(address);
        }
    }
}

fn lock(callers: &Callers) -> std::sync::MutexGuard<'_, Tally> {
    match callers.lock() {
        Ok(callers) => callers,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// An accepted connection, holding its place until dropped, and failing
/// once nothing has crossed it for too long.
#[derive(Debug)]
pub struct Held {
    _caller: Caller,
    _permit: OwnedSemaphorePermit,
    deadline: Pin<Box<Sleep>>,
    idle: Duration,
    /// Whether anything was written back yet: until then, the caller is
    /// still sending its first request, held to [`QUIET`].
    answered: bool,
    stream: TcpStream,
}

impl Held {
    fn new(
        stream: TcpStream,
        permit: OwnedSemaphorePermit,
        caller: Caller,
        idle: Duration,
    ) -> Self {
        Self {
            _caller: caller,
            _permit: permit,
            deadline: Box::pin(sleep(QUIET)),
            idle,
            answered: false,
            stream,
        }
    }

    /// Something crossed: push the deadline back, once the relay has
    /// answered. Until then the deadline set at accepting stands.
    fn crossed(&mut self) {
        if self.answered {
            self.deadline.as_mut().reset(Instant::now() + self.idle);
        }
    }

    fn expired(&mut self, context: &mut Context<'_>) -> bool {
        self.deadline.as_mut().poll(context).is_ready()
    }
}

fn timed_out() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "nothing crossed the connection")
}

impl AsyncRead for Held {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let before = buffer.filled().len();
        match Pin::new(&mut self.stream).poll_read(context, buffer) {
            Poll::Ready(result) => {
                if buffer.filled().len() > before {
                    self.crossed();
                }
                Poll::Ready(result)
            }
            Poll::Pending if self.expired(context) => Poll::Ready(Err(timed_out())),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl AsyncWrite for Held {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let written = Pin::new(&mut self.stream).poll_write(context, bytes);
        if let Poll::Ready(Ok(count)) = written
            && count > 0
        {
            self.answered = true;
            self.crossed();
        }
        written
    }

    fn poll_flush(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(context)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(context)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffers: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let written = Pin::new(&mut self.stream).poll_write_vectored(context, buffers);
        if let Poll::Ready(Ok(count)) = written
            && count > 0
        {
            self.answered = true;
            self.crossed();
        }
        written
    }

    fn is_write_vectored(&self) -> bool {
        self.stream.is_write_vectored()
    }
}
