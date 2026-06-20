use anyhow::Context;
use codec::{Pdu, SetClientId};
use config::ConfigHandle;
use mux::client::ClientId;
use std::ffi::OsString;
use std::io::{Read, Write};
use std::process::Command;
use wezterm_client::client::unix_connect_with_retry;

pub fn run(
    config: &ConfigHandle,
    skip_config: bool,
    config_file: Option<&OsString>,
    config_override: &[(String, String)],
) -> anyhow::Result<()> {
    let unix_dom = config
        .unix_domains
        .first()
        .ok_or_else(|| anyhow::anyhow!("no unix domains configured"))?;
    let target = unix_dom.target();

    let mut stream = match unix_connect_with_retry(&target, false, Some(3)) {
        Ok(s) => s,
        Err(_) => {
            // Server is not running; start it in the background and retry.
            let mut cmd =
                Command::new(std::env::current_exe().context("resolving current exe")?);
            cmd.arg("--daemonize");
            // Redirect stdin/stdout to /dev/null so the daemon process does not
            // inherit this process's SSH tunnel file descriptors.
            cmd.stdin(std::process::Stdio::null());
            cmd.stdout(std::process::Stdio::null());
            if skip_config {
                cmd.arg("-n");
            }
            if let Some(f) = config_file {
                cmd.arg("--config-file");
                cmd.arg(f);
            }
            for (name, value) in config_override {
                cmd.arg("--config");
                cmd.arg(format!("{name}={value}"));
            }
            cmd.spawn().context("spawning wezterm-mux-server --daemonize")?;
            unix_connect_with_retry(&target, true, None)
                .context("connecting to mux server after daemon start")?
        }
    };

    // Handshake: identify ourselves as a proxy connection.
    let pdu = Pdu::SetClientId(SetClientId {
        client_id: ClientId::new(),
        is_proxy: true,
    });
    pdu.encode(&mut stream, 1)?;
    Pdu::decode(&mut stream)?;

    // Bridge stdin→socket and socket→stdout in two threads.
    // Each thread calls process::exit on EOF so either side can terminate cleanly.
    let reader = stream.try_clone()?;
    std::thread::spawn(move || {
        let stdout = std::io::stdout();
        copy_until_eof_then_exit(reader, stdout.lock());
    });
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        copy_until_eof_then_exit(stdin.lock(), stream);
    });

    // Park the main thread; the bridge threads above call process::exit on EOF.
    loop {
        std::thread::park();
    }
}

fn copy_until_eof_then_exit<R: Read, W: Write>(mut from: R, mut to: W) -> ! {
    let mut buf = [0u8; 8192];
    loop {
        match from.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if to.write_all(&buf[..n]).is_err() {
                    break;
                }
                to.flush().ok();
            }
        }
    }
    // Brief pause so the peer thread can flush before we exit.
    std::thread::sleep(std::time::Duration::from_millis(100));
    std::process::exit(0);
}
