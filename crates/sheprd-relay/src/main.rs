//! `sheprd-relay`: see the library.

use sheprd_relay::{Options, Result, serve};
use std::process::ExitCode;

fn main() -> ExitCode {
    let options = match Options::parse(std::env::args_os().skip(1), |name| std::env::var_os(name)) {
        Ok(Some(options)) => options,
        Ok(None) => {
            println!("{}", sheprd_relay::USAGE);
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            eprintln!("{error}\n\n{}", sheprd_relay::USAGE);
            return ExitCode::from(2);
        }
    };
    tracing_subscriber::fmt().with_target(false).init();
    match run(options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "the relay stopped");
            ExitCode::FAILURE
        }
    }
}

fn run(options: Options) -> Result<()> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(sheprd_relay::Error::Runtime)?
        .block_on(serve(options))
}
