use anyhow::Context;
use codec::{Pdu, SetClientId};
use config::ConfigHandle;
use mux::client::ClientId;
use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::Path;
use std::process::Command;
use wezterm_client::client::unix_connect_with_retry;

pub fn run(
    config: &ConfigHandle,
    skip_config: bool,
    config_file: Option<&OsString>,
    config_override: &[(String, String)],
    replace: bool,
) -> anyhow::Result<()> {
    // Announce our own digest on stderr so that a client whose connection then
    // fails can decide whether uploading a fresh binary would help. Hashed on a
    // background thread so reading our (large) executable doesn't delay the
    // connection on the happy path.
    std::thread::spawn(report_self_digest);

    let unix_dom = config
        .unix_domains
        .first()
        .ok_or_else(|| anyhow::anyhow!("no unix domains configured"))?;
    let target = unix_dom.target();
    let sock_path = unix_dom.socket_path();

    // When asked to replace, terminate any daemon already listening on this
    // socket so the connection below starts a fresh one from *this* binary.
    if replace {
        replace_running_daemon(config, &sock_path);
    }

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

/// Compute this executable's SHA-256 and print it to stderr in a form the
/// client can recognize. Best-effort: errors are logged and ignored.
fn report_self_digest() {
    match self_digest() {
        Ok(hex) => eprintln!("WEZTERM_MUX_DIGEST=sha256:{hex}"),
        Err(err) => log::warn!("could not compute mux-server self digest: {err:#}"),
    }
}

/// SHA-256 of our own executable, as lowercase hex. Uses the OpenSSL library
/// (already linked for TLS), matching the digest the client computes for the
/// binary it would upload, with no `openssl`/`sha256sum` CLI needed on the
/// remote.
fn self_digest() -> anyhow::Result<String> {
    let exe = std::env::current_exe().context("resolving current exe")?;
    let data = std::fs::read(&exe).with_context(|| format!("reading {}", exe.display()))?;
    let digest = openssl::hash::hash(openssl::hash::MessageDigest::sha256(), &data)
        .context("hashing current exe")?;
    Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
}

/// Terminate a mux-server daemon already listening on `sock_path` so a fresh
/// one can be started from the current binary. No-op when nothing is listening.
/// The peer pid comes from `SO_PEERCRED` on the connection we open (Linux),
/// falling back to the pid file. This ends the daemon's sessions, which is
/// acceptable: a client couldn't have reattached to an incompatible daemon
/// anyway.
fn replace_running_daemon(config: &ConfigHandle, sock_path: &Path) {
    use std::os::unix::net::UnixStream;

    let pid = match UnixStream::connect(sock_path) {
        Ok(stream) => peer_pid(&stream).or_else(|| read_pid_file(config)),
        // Nothing listening; nothing to replace.
        Err(_) => return,
    };

    let Some(pid) = pid else {
        log::warn!(
            "--replace: a daemon is listening on {} but its pid is unknown; not killing it",
            sock_path.display()
        );
        return;
    };

    log::warn!("--replace: terminating existing mux-server (pid {pid}); this ends its sessions");
    unsafe {
        libc::kill(pid, libc::SIGTERM);
    }
    wait_for_daemon_exit(sock_path);
}

/// Return the pid of the process on the other end of a connected unix socket.
#[cfg(target_os = "linux")]
fn peer_pid(stream: &std::os::unix::net::UnixStream) -> Option<i32> {
    use std::os::unix::io::AsRawFd;
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut libc::ucred as *mut libc::c_void,
            &mut len,
        )
    };
    (rc == 0 && cred.pid > 0).then_some(cred.pid)
}

#[cfg(not(target_os = "linux"))]
fn peer_pid(_stream: &std::os::unix::net::UnixStream) -> Option<i32> {
    None
}

fn read_pid_file(config: &ConfigHandle) -> Option<i32> {
    let pid_file = config.daemon_options.pid_file();
    std::fs::read_to_string(&pid_file)
        .ok()?
        .trim()
        .parse::<i32>()
        .ok()
}

/// Wait (bounded) for the old daemon to stop accepting connections. It exits on
/// SIGTERM, and the next daemon unlinks any stale socket before binding, so
/// once connecting fails we're safe to start a fresh one.
fn wait_for_daemon_exit(sock_path: &Path) {
    use std::os::unix::net::UnixStream;
    for _ in 0..50 {
        if UnixStream::connect(sock_path).is_err() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    log::warn!("--replace: timed out waiting for the old mux-server to exit");
}
