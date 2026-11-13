//! Checks a relay list as clients will: `relays LIST [--asc FILE] [--sig
//! FILE]`. Prints who signed it and what it lists, or why a client would
//! refuse it, and exits non-zero then. `just relays-sign` runs it after
//! signing.

use sheprd_relay::directory::{
    Directory,
    signed::{Signatures, Signed},
};
use std::{
    path::PathBuf,
    process::ExitCode,
    time::{SystemTime, UNIX_EPOCH},
};

fn main() -> ExitCode {
    match run() {
        Ok(signed) => {
            println!(
                "relays.json: serial {}, expires {}, validated by {}",
                signed.serial,
                signed.expires,
                signed.signer.describe()
            );
            for relay in &signed.directory.relays {
                let pin = if relay.quic_pin.is_some() {
                    "QUIC pin"
                } else {
                    "no QUIC pin"
                };
                println!("  {} ({pin})", relay.endpoint);
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("relays.json would be refused: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> anyhow::Result<Signed> {
    let mut arguments = std::env::args_os().skip(1);
    let list: PathBuf = arguments
        .next()
        .ok_or_else(|| anyhow::anyhow!("usage: relays LIST [--asc FILE] [--sig FILE]"))?
        .into();
    let (mut asc, mut sig) = (None, None);
    while let Some(flag) = arguments.next() {
        let file: PathBuf = arguments
            .next()
            .ok_or_else(|| anyhow::anyhow!("{} needs a file", flag.to_string_lossy()))?
            .into();
        match flag.to_str() {
            Some("--asc") => asc = Some(std::fs::read(file)?),
            Some("--sig") => sig = Some(std::fs::read(file)?),
            _ => anyhow::bail!("unknown option {}", flag.to_string_lossy()),
        }
    }
    let bytes = std::fs::read(&list)?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let signatures = Signatures {
        openpgp: asc.as_deref(),
        ed25519: sig.as_deref(),
    };
    Ok(Directory::verify(&bytes, signatures, now, None)?)
}
