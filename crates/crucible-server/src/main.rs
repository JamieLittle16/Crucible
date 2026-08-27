use std::env;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use crucible_server::{
    DEFAULT_R0_BIND_ADDRESS, R0_ADMISSION_SESSION_SHA256, R0_ORACLE_STATUS_JSON,
    ServerSessionEpoch, load_r1x_image, load_r2b_playtest_image, serve_r0_blocking_transport,
    serve_r1a_blocking_transport, serve_r1x_blocking_transport,
    serve_r2b_playtest_blocking_transport,
};
use crucible_target_26_2::generated;

const CONNECTION_TIMEOUT: Duration = Duration::from_secs(15);
const LOGIN_SESSION_EPOCH_PREFIX: &str = "--login-session-epoch=";
const R1X_REPLAY_IMAGE_PREFIX: &str = "--r1x-replay-image=";
const R2B_PLAYTEST_IMAGE_PREFIX: &str = "--r2b-playtest-image=";
const R1X_CAPTURE_SESSION_EPOCH_HEX: &str = "4d7f604f196a43b08987f0b2a27c2663";

#[derive(Debug)]
struct Options {
    bind_address: String,
    once: bool,
    login_session_epoch: Option<ServerSessionEpoch>,
    r1x_replay_image: Option<PathBuf>,
    r2b_playtest_image: Option<PathBuf>,
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
    let r1x_context = options
        .r1x_replay_image
        .as_deref()
        .map(|path| {
            load_r1x_image(path, R0_ORACLE_STATUS_JSON).map_err(|error| {
                format!(
                    "could not load R1X replay image {}: {error}",
                    path.display()
                )
            })
        })
        .transpose()?;
    let r2b_playtest = options
        .r2b_playtest_image
        .as_deref()
        .map(|path| {
            load_r2b_playtest_image(path, R0_ORACLE_STATUS_JSON).map_err(|error| {
                format!(
                    "could not load R2B playtest image {}: {error}",
                    path.display()
                )
            })
        })
        .transpose()?;

    let listener = TcpListener::bind(&options.bind_address)
        .map_err(|error| format!("could not bind {}: {error}", options.bind_address))?;
    let local_address = listener
        .local_addr()
        .map_err(|error| format!("could not read listener address: {error}"))?;

    if r2b_playtest.is_some() {
        println!(
            "Crucible R2B playtest listening on {local_address} | Minecraft {} protocol {} | real Configuration | replay-free Play bootstrap | captured Play publication 0 | R2C world projection pending | production_admitted=false",
            generated::login_26_2::MINECRAFT_VERSION,
            generated::login_26_2::PROTOCOL_VERSION,
        );
    } else if let Some(context) = r1x_context.as_ref() {
        println!(
            "Crucible R1X listening on {local_address} | Minecraft {} protocol {} | Configuration admitted | experimental Play frames {} ({} body bytes) | production_admitted=false",
            generated::login_26_2::MINECRAFT_VERSION,
            generated::login_26_2::PROTOCOL_VERSION,
            context.play_frame_count(),
            context.play_body_bytes(),
        );
    } else if options.login_session_epoch.is_some() {
        println!(
            "Crucible R1A listening on {local_address} | Minecraft {} protocol {} | login contract {} | Configuration intentionally not yet admitted",
            generated::login_26_2::MINECRAFT_VERSION,
            generated::login_26_2::PROTOCOL_VERSION,
            generated::login_26_2::CONTRACT_ID,
        );
    } else {
        println!(
            "Crucible R0 listening on {local_address} | Minecraft {} protocol {} | contract {} | session {}",
            generated::MINECRAFT_VERSION,
            generated::PROTOCOL_VERSION,
            generated::CONTRACT_ID,
            R0_ADMISSION_SESSION_SHA256,
        );
    }

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

        if let Some(image) = r2b_playtest.as_ref() {
            let session_epoch = options
                .login_session_epoch
                .expect("R2B playtest option normalization supplies the fixed capture epoch");
            match serve_r2b_playtest_blocking_transport(&mut stream, session_epoch, image) {
                Ok(exit) => eprintln!("R2B playtest connection {peer} completed: {exit:?}"),
                Err(error) if !options.once => {
                    eprintln!("R2B playtest connection {peer} rejected: {error:?}");
                }
                Err(error) => {
                    return Err(format!("R2B playtest connection {peer} failed: {error:?}"));
                }
            }
        } else if let Some(context) = r1x_context.as_ref() {
            let session_epoch = options
                .login_session_epoch
                .expect("R1X option normalization supplies the fixed capture epoch");
            match serve_r1x_blocking_transport(&mut stream, session_epoch, context) {
                Ok(exit) => eprintln!("R1X connection {peer} completed: {exit:?}"),
                Err(error) if !options.once => {
                    eprintln!("R1X connection {peer} rejected: {error:?}");
                }
                Err(error) => return Err(format!("R1X connection {peer} failed: {error:?}")),
            }
        } else if let Some(session_epoch) = options.login_session_epoch {
            match serve_r1a_blocking_transport(&mut stream, session_epoch) {
                Ok(exit) => eprintln!("R1A connection {peer} completed: {exit:?}"),
                Err(error) if !options.once => {
                    eprintln!("R1A connection {peer} rejected: {error:?}");
                }
                Err(error) => return Err(format!("R1A connection {peer} failed: {error:?}")),
            }
        } else {
            match serve_r0_blocking_transport(&mut stream) {
                Ok(exit) => eprintln!("R0 connection {peer} completed: {exit:?}"),
                Err(error) if !options.once => {
                    eprintln!("R0 connection {peer} rejected: {error:?}");
                }
                Err(error) => return Err(format!("R0 connection {peer} failed: {error:?}")),
            }
        }

        if options.once {
            return Ok(());
        }
    }
}

fn options() -> Result<Options, String> {
    let mut bind_address = None;
    let mut once = false;
    let mut login_session_epoch = None;
    let mut r1x_replay_image = None;
    let mut r2b_playtest_image = None;

    for argument in env::args().skip(1) {
        if argument == "--once" {
            if once {
                return Err("--once may be supplied only once".to_owned());
            }
            once = true;
        } else if let Some(value) = argument.strip_prefix(LOGIN_SESSION_EPOCH_PREFIX) {
            let epoch = ServerSessionEpoch::parse_hex(value)
                .map_err(|error| format!("invalid --login-session-epoch: {error}"))?;
            if login_session_epoch.replace(epoch).is_some() {
                return Err("--login-session-epoch may be supplied only once".to_owned());
            }
        } else if let Some(value) = argument.strip_prefix(R1X_REPLAY_IMAGE_PREFIX) {
            if value.is_empty() {
                return Err("--r1x-replay-image requires a non-empty path".to_owned());
            }
            if r1x_replay_image.replace(PathBuf::from(value)).is_some() {
                return Err("--r1x-replay-image may be supplied only once".to_owned());
            }
        } else if let Some(value) = argument.strip_prefix(R2B_PLAYTEST_IMAGE_PREFIX) {
            if value.is_empty() {
                return Err("--r2b-playtest-image requires a non-empty path".to_owned());
            }
            if r2b_playtest_image.replace(PathBuf::from(value)).is_some() {
                return Err("--r2b-playtest-image may be supplied only once".to_owned());
            }
        } else if argument.starts_with('-') {
            return Err(format!("unknown option {argument:?}"));
        } else if bind_address.replace(argument).is_some() {
            return Err("at most one bind address may be supplied".to_owned());
        }
    }

    if r1x_replay_image.is_some() && r2b_playtest_image.is_some() {
        return Err("--r1x-replay-image and --r2b-playtest-image are mutually exclusive".to_owned());
    }

    if r1x_replay_image.is_some() || r2b_playtest_image.is_some() {
        let capture_epoch = ServerSessionEpoch::parse_hex(R1X_CAPTURE_SESSION_EPOCH_HEX)
            .expect("committed capture session epoch is valid");
        match login_session_epoch {
            Some(epoch) if epoch != capture_epoch => {
                return Err(format!(
                    "capture-qualified R1X/R2B development routes require --login-session-epoch={R1X_CAPTURE_SESSION_EPOCH_HEX}"
                ));
            }
            Some(_) => {}
            None => login_session_epoch = Some(capture_epoch),
        }
    }

    Ok(Options {
        bind_address: bind_address.unwrap_or_else(|| DEFAULT_R0_BIND_ADDRESS.to_owned()),
        once,
        login_session_epoch,
        r1x_replay_image,
        r2b_playtest_image,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_R0_BIND_ADDRESS, LOGIN_SESSION_EPOCH_PREFIX, R1X_CAPTURE_SESSION_EPOCH_HEX,
        R1X_REPLAY_IMAGE_PREFIX, R2B_PLAYTEST_IMAGE_PREFIX, options,
    };

    #[test]
    fn default_bind_address_remains_localhost() {
        assert_eq!(DEFAULT_R0_BIND_ADDRESS, "127.0.0.1:25565");
    }

    #[test]
    fn option_parser_shape_is_kept_out_of_protocol_semantics() {
        let _ = options as fn() -> Result<super::Options, String>;
        assert_eq!(LOGIN_SESSION_EPOCH_PREFIX, "--login-session-epoch=");
        assert_eq!(R1X_REPLAY_IMAGE_PREFIX, "--r1x-replay-image=");
        assert_eq!(R2B_PLAYTEST_IMAGE_PREFIX, "--r2b-playtest-image=");
        assert_eq!(R1X_CAPTURE_SESSION_EPOCH_HEX.len(), 32);
    }
}
