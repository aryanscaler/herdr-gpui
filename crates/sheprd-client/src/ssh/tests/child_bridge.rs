//! The bridge over a child's inherited stdio: anonymous pipes on Windows,
//! a socket pair on Unix. A native fixture avoids PowerShell's cold startup.
use super::*;

mod fixture;

const COMPATIBLE: &str = r#"{"endpoint_protocol_generation":1,"endpoint_capabilities":["surface_interest","presentation_effects_fence","health_check"],"remote_bridge_idle_timeout":true}"#;

/// A bound for waits that only a broken fixture would reach.
const STARTUP: Duration = Duration::from_secs(30);

fn timed_out(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::TimedOut
        || (cfg!(unix) && error.kind() == io::ErrorKind::WouldBlock)
}

/// Reads exactly `expected.len()` bytes, retrying the emulated timeouts an
/// idle pipe reports, and returns them.
fn read_expected(stream: &mut Stream, expected: &[u8]) -> Vec<u8> {
    let deadline = Instant::now() + STARTUP;
    let mut read = vec![0; expected.len()];
    let mut filled = 0;
    while filled < read.len() {
        assert!(Instant::now() < deadline, "timed out after {filled} bytes");
        match stream.read(&mut read[filled..]) {
            Ok(0) => panic!("closed after {filled} bytes"),
            Ok(n) => filled += n,
            Err(error) if timed_out(&error) => {}
            Err(error) => panic!("{error}"),
        }
    }
    read
}

#[test]
fn the_handshake_and_the_bridged_stream_run_over_the_childs_pipes() {
    let stop = AtomicBool::new(false);
    let Ok((mut stream, _child)) =
        start_bridge(fixture::command("handshake"), &stop, |stream, stop| {
            fixture::await_start(stream);
            handshake(stream, stop)
        })
        .unwrap()
    else {
        panic!("the bridge closed before it was ready");
    };
    stream.write_all(b"hello\n").unwrap();
    assert_eq!(read_expected(&mut stream, b"hello\n"), b"hello\n");
    // Only the child held the pipes' other ends, so its exit reads as the end
    // of the stream rather than as one more idle timeout.
    let deadline = Instant::now() + STARTUP;
    let mut byte = [0];
    loop {
        assert!(Instant::now() < deadline, "the exit never read as closed");
        match stream.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => panic!("unexpected output"),
            Err(error) if timed_out(&error) => {}
            Err(error) => panic!("{error}"),
        }
    }
}

#[test]
fn an_ssh_failure_before_the_banner_is_classified_from_its_stderr() {
    let command = fixture::command("failure");
    let mut answered = None;
    let Err(closed) = start_bridge(command, &AtomicBool::new(false), |stream, stop| {
        fixture::await_start(stream);
        answered = Some(Instant::now());
        handshake(stream, stop)
    })
    .unwrap() else {
        panic!("a failed ssh was accepted");
    };
    assert_eq!(closed.code, Some(255));
    assert_eq!(closed.failure, SshFailure::HostKey);
    // Reported when the pipe closed, well before the banner's deadline.
    assert!(answered.unwrap().elapsed() < Duration::from_secs(10));
}

#[test]
fn an_idle_pipe_times_out_rather_than_closing_and_cancellation_ends_the_wait() {
    // Half a ready line, then nothing until stdin closes.
    let (mut stream, child) = spawn_bridge(fixture::command("partial")).unwrap();
    stream.set_read_timeout(Some(POLL)).unwrap();
    fixture::await_start(&mut stream);
    assert_eq!(read_expected(&mut stream, b"herdr-remote"), b"herdr-remote");
    let mut byte = [0];
    let error = stream.read(&mut byte).unwrap_err();
    assert!(timed_out(&error), "{error}");
    // The banner never finishes; cancelling returns without the deadline.
    let stop = AtomicBool::new(true);
    assert!(matches!(
        await_ready(&mut stream, &stop, Instant::now()),
        Err(crate::Error::Ssh(Error::Cancelled))
    ));
    drop(child);
}

#[test]
fn a_banner_without_a_ready_line_is_bounded() {
    let (mut stream, _child) = spawn_bridge(fixture::command("oversize")).unwrap();
    stream.set_read_timeout(Some(POLL)).unwrap();
    fixture::await_start(&mut stream);
    assert!(matches!(
        await_ready(&mut stream, &AtomicBool::new(false), Instant::now()),
        Err(crate::Error::Ssh(Error::OutputLimit))
    ));
}
