use super::*;
use std::io::BufRead;

const MODE: &str = "SHEPRD_TEST_BRIDGE_MODE";
const STARTED: &[u8] = b"\nsheprd-test-bridge-started\n";

pub(super) fn command(mode: &str) -> Command {
    let mut command = crate::new_command(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "ssh::tests::child_bridge::fixture::child",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(MODE, mode);
    command
}

/// Discard only libtest's preamble; bytes after the marker belong to the bridge.
/// The child's startup is separate from the protocol's own banner deadline.
pub(super) fn await_start(stream: &mut Stream) {
    let deadline = Instant::now() + STARTUP;
    let mut preamble = Vec::new();
    let mut byte = [0];
    while !preamble.ends_with(STARTED) {
        assert!(
            Instant::now() < deadline,
            "fixture never started: {preamble:?}"
        );
        assert!(preamble.len() < 4096, "fixture preamble is unbounded");
        match stream.read(&mut byte) {
            Ok(0) => panic!("fixture closed before starting: {preamble:?}"),
            Ok(_) => preamble.push(byte[0]),
            Err(error) if timed_out(&error) => {}
            Err(error) => panic!("{error}"),
        }
    }
}

#[test]
fn child() {
    let Ok(mode) = std::env::var(MODE) else {
        return;
    };
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    output.write_all(STARTED).unwrap();
    output.flush().unwrap();
    let mut line = String::new();
    match mode.as_str() {
        "handshake" => {
            write!(output, "motd\n{COMPATIBLE}\n\n").unwrap();
            output.write_all(READY).unwrap();
            output.flush().unwrap();
            input.read_line(&mut line).unwrap();
            assert_eq!(line, "accept-idle\n");
            line.clear();
            input.read_line(&mut line).unwrap();
            output.write_all(line.as_bytes()).unwrap();
            output.flush().unwrap();
        }
        "partial" | "oversize" => {
            let bytes = if mode == "partial" {
                b"herdr-remote".to_vec()
            } else {
                vec![b'x'; 20000]
            };
            output.write_all(&bytes).unwrap();
            output.flush().unwrap();
            input.read_line(&mut line).unwrap();
        }
        "failure" => {
            let mut error = io::stderr().lock();
            error.write_all(b"Host key verification failed.\n").unwrap();
            error.flush().unwrap();
            std::process::exit(255);
        }
        _ => panic!("unknown fixture mode: {mode}"),
    }
    // Do not let libtest's result summary become trailing protocol bytes.
    std::process::exit(0);
}
