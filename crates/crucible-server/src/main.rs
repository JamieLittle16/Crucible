use std::env;
use std::net::TcpListener;
use std::process::ExitCode;
use std::time::Duration;

use crucible_server::{
    DEFAULT_R0_BIND_ADDRESS, R0_ADMISSION_SESSION_SHA256, serve_r0_blocking_transport,
};
use crucible_target_26_2::generated;

const CONNECTION_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug)]
struct Options {
    bind_address: String,
    once: bool,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("crucible-server: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let options = options()?;
    let listener = TcpListener::bind(&options.bind_address)
        .map_err(|error| format!("could not bind {}: {error}", options.bind_address))?;
    let local_address = listener
        .local_addr()
        .map_err(|error| format!("could not read listener address: {error}"))?;

    println!(
        "Crucible R0 listening on {local_address} | Minecraft {} protocol {} | contract {} | session {}",
        generated::MINECRAFT_VERSION,
        generated::PROTOCOL_VERSION,
        generated::CONTRACT_ID,
        R0_ADMISSION_SESSION_SHA256,
    );

    loop {
        let (mut stream, peer) = listener
            .accept()
            .map_err(|error| format!("accept failed: {error}"))?;
        stream
            .set_read_timeout(Some(CONNECTION_TIMEOUT))
            .map_err(|error| format!("could not set read timeout for {peer}: {error}"))?;
        stream
            .set_write_timeout(Some(CONNECTION_TIMEOUT))
            .map_err(|error| format!("could not set write timeout for {peer}: {error}"))?;
        stream
            .set_nodelay(true)
            .map_err(|error| format!("could not enable TCP_NODELAY for {peer}: {error}"))?;

        match serve_r0_blocking_transport(&mut stream) {
            Ok(exit) => eprintln!("R0 connection {peer} completed: {exit:?}"),
            Err(error) if !options.once => {
                eprintln!("R0 connection {peer} rejected: {error:?}");
            }
            Err(error) => return Err(format!("R0 connection {peer} failed: {error:?}")),
        }

        if options.once {
            return Ok(());
        }
    }
}

fn options() -> Result<Options, String> {
    let mut bind_address = None;
    let mut once = false;

    for argument in env::args().skip(1) {
        if argument == "--once" {
            if once {
                return Err("--once may be supplied only once".to_owned());
            }
            once = true;
        } else if argument.starts_with('-') {
            return Err(format!("unknown option {argument:?}"));
        } else if bind_address.replace(argument).is_some() {
            return Err("at most one bind address may be supplied".to_owned());
        }
    }

    Ok(Options {
        bind_address: bind_address.unwrap_or_else(|| DEFAULT_R0_BIND_ADDRESS.to_owned()),
        once,
    })
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_R0_BIND_ADDRESS, options};

    #[test]
    fn default_bind_address_remains_localhost() {
        assert_eq!(DEFAULT_R0_BIND_ADDRESS, "127.0.0.1:25565");
    }

    #[test]
    fn option_parser_shape_is_kept_out_of_protocol_semantics() {
        let _ = options as fn() -> Result<super::Options, String>;
    }
}
