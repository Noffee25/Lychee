//! One Lychee at a time. A second launch (say, "Open with Lychee" in a file manager) hands its
//! files to the running window and exits, instead of starting another player on the same library.
//!
//! The first instance holds an exclusive lock on `instance.lock` in its data folder and listens on
//! a local socket: a Unix socket in the runtime folder, or on Windows a loopback TCP port written
//! next to the lock together with a random key. Later launches find the lock taken, send their
//! request and quit. A crashed instance leaves nothing that matters behind: the OS drops its lock.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};

/// What a launch asks of the running Lychee. Nothing to open means "show yourself".
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Request {
    /// Files and folders to play.
    pub open: Vec<PathBuf>,
    /// Files and folders to open in the tag editor.
    pub edit: Vec<PathBuf>,
}

pub enum Claim {
    /// This is the only Lychee: requests from later launches arrive on `Primary::requests`.
    Primary(Primary),
    /// Another Lychee is running and took the request.
    Forwarded,
}

/// Held by the running Lychee for as long as it runs.
pub struct Primary {
    pub requests: Receiver<Request>,
    _lock: File,
    endpoint: PathBuf,
    closing: Arc<AtomicBool>,
}

impl Primary {
    /// Stop taking requests (call when shutting down). Later launches then wait for the lock and
    /// start afresh once this Lychee has finished saving, instead of handing their files to a
    /// player that is going away.
    pub fn close(&self) {
        if !self.closing.swap(true, Ordering::SeqCst) {
            let _ = std::fs::remove_file(&self.endpoint);
        }
    }
}

impl Drop for Primary {
    fn drop(&mut self) {
        self.close();
    }
}

const MAGIC: &[u8] = b"lychee-1";
/// How long a new launch keeps trying to reach a Lychee that is still starting up, or to take
/// over from one that is shutting down.
const PATIENCE: Duration = Duration::from_secs(10);
const IO_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_REQUEST: u64 = 4 << 20;

/// Become the running Lychee for the data folder `dir`, or hand `request` to the one that is.
/// Relative paths in `request` should already be absolute (the running Lychee has its own
/// working directory). An error means neither worked; the caller can run on its own.
pub fn claim(dir: &Path, request: &Request) -> io::Result<Claim> {
    std::fs::create_dir_all(dir)?;
    let lock = OpenOptions::new().create(true).truncate(false).read(true).write(true).open(dir.join("instance.lock"))?;
    let started = Instant::now();
    loop {
        match lock.try_lock() {
            Ok(()) => {
                let (tx, rx) = crossbeam_channel::unbounded();
                let closing = Arc::new(AtomicBool::new(false));
                let endpoint = listen(dir, tx, closing.clone())?;
                return Ok(Claim::Primary(Primary { requests: rx, _lock: lock, endpoint, closing }));
            }
            Err(std::fs::TryLockError::WouldBlock) => match send(dir, request) {
                Ok(()) => return Ok(Claim::Forwarded),
                // The running Lychee may still be starting (its socket comes up right after the
                // lock) or shutting down (then the lock frees up soon): try both again.
                Err(e) if started.elapsed() < PATIENCE => {
                    tracing::debug!("waiting for the running Lychee: {e}");
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(e) => return Err(e),
            },
            Err(std::fs::TryLockError::Error(e)) => return Err(e),
        }
    }
}

/// A request on the wire: the magic, the key, then each path, all ending in a NUL byte. Paths
/// to edit are marked with a leading `e`, paths to play with `o`.
fn encode(key: &str, request: &Request) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.push(0);
    out.extend_from_slice(key.as_bytes());
    out.push(0);
    for (mark, paths) in [(b'o', &request.open), (b'e', &request.edit)] {
        for p in paths {
            out.push(mark);
            out.extend_from_slice(&path_bytes(p));
            out.push(0);
        }
    }
    out
}

fn decode(key: &str, bytes: &[u8]) -> Option<Request> {
    let mut fields = bytes.split(|b| *b == 0);
    if fields.next()? != MAGIC || fields.next()? != key.as_bytes() {
        return None;
    }
    let mut request = Request::default();
    for field in fields {
        match field.split_first() {
            Some((b'o', path)) => request.open.push(bytes_path(path)),
            Some((b'e', path)) => request.edit.push(bytes_path(path)),
            _ => {}
        }
    }
    Some(request)
}

#[cfg(unix)]
fn path_bytes(p: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    p.as_os_str().as_bytes().to_vec()
}

#[cfg(unix)]
fn bytes_path(b: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::OsStr::from_bytes(b).into()
}

#[cfg(not(unix))]
fn path_bytes(p: &Path) -> Vec<u8> {
    p.to_string_lossy().into_owned().into_bytes()
}

#[cfg(not(unix))]
fn bytes_path(b: &[u8]) -> PathBuf {
    String::from_utf8_lossy(b).into_owned().into()
}

/// Read one request from a connection, hand it on and confirm. Once the app is closing, requests
/// go unanswered, so the sender tries again (and takes over when this Lychee is gone).
fn serve(mut stream: impl Read + Write, key: &str, tx: &Sender<Request>, closing: &AtomicBool) -> io::Result<()> {
    if closing.load(Ordering::SeqCst) {
        return Err(io::Error::new(io::ErrorKind::BrokenPipe, "shutting down"));
    }
    let mut bytes = Vec::new();
    (&mut stream).take(MAX_REQUEST).read_to_end(&mut bytes)?;
    let request = decode(key, &bytes).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "not a Lychee request"))?;
    if closing.load(Ordering::SeqCst) {
        return Err(io::Error::new(io::ErrorKind::BrokenPipe, "shutting down"));
    }
    tracing::info!("another launch asked for {:?}", request);
    tx.send(request).map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "shutting down"))?;
    stream.write_all(b"ok")
}

/// Send a request, say that's all (`end_request` shuts down the writing side) and wait for the
/// confirmation.
fn exchange(mut stream: impl Read + Write, key: &str, request: &Request, end_request: impl FnOnce()) -> io::Result<()> {
    stream.write_all(&encode(key, request))?;
    end_request();
    let mut reply = [0u8; 2];
    stream.read_exact(&mut reply)?;
    if &reply == b"ok" { Ok(()) } else { Err(io::Error::new(io::ErrorKind::InvalidData, "unexpected reply")) }
}

// --- Unix: a socket in the runtime folder ----------------------------------------------------

#[cfg(unix)]
fn socket_path(dir: &Path) -> PathBuf {
    // Socket paths are limited to about 100 bytes, so the socket lives in the (short, private)
    // runtime folder, named after the data folder it belongs to.
    let base = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).filter(|d| d.is_dir()).unwrap_or_else(std::env::temp_dir);
    let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    base.join(format!("lychee-{:016x}.sock", fnv1a(&path_bytes(&dir))))
}

#[cfg(unix)]
fn listen(dir: &Path, tx: Sender<Request>, closing: Arc<AtomicBool>) -> io::Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;
    let path = socket_path(dir);
    // We hold the lock, so a socket file left here is a crashed Lychee's.
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    std::thread::Builder::new().name("lychee-instance".into()).spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
            let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
            match serve(&stream, "", &tx, &closing) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::BrokenPipe => break,
                Err(e) => tracing::warn!("ignoring a request from another launch: {e}"),
            }
        }
    })?;
    Ok(path)
}

#[cfg(unix)]
fn send(dir: &Path, request: &Request) -> io::Result<()> {
    use std::os::unix::net::UnixStream;
    let stream = UnixStream::connect(socket_path(dir))?;
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    exchange(&stream, "", request, || {
        let _ = stream.shutdown(std::net::Shutdown::Write);
    })
}

// --- Windows: a loopback port and a key, in a file only this user can read --------------------

#[cfg(not(unix))]
fn listen(dir: &Path, tx: Sender<Request>, closing: Arc<AtomicBool>) -> io::Result<PathBuf> {
    use std::net::{Ipv4Addr, TcpListener};
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let port = listener.local_addr()?.port();
    let key = random_key();
    let path = dir.join("instance.port");
    std::fs::write(&path, format!("{port} {key}"))?;
    std::thread::Builder::new().name("lychee-instance".into()).spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
            let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
            match serve(&stream, &key, &tx, &closing) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::BrokenPipe => break,
                Err(e) => tracing::warn!("ignoring a request from another launch: {e}"),
            }
        }
    })?;
    Ok(path)
}

#[cfg(not(unix))]
fn send(dir: &Path, request: &Request) -> io::Result<()> {
    use std::net::{Ipv4Addr, TcpStream};
    let text = std::fs::read_to_string(dir.join("instance.port"))?;
    let (port, key) = text.trim().split_once(' ').ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad port file"))?;
    let port: u16 = port.parse().map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "bad port file"))?;
    let stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port))?;
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    exchange(&stream, key, request, || {
        let _ = stream.shutdown(std::net::Shutdown::Write);
    })
}

#[cfg(not(unix))]
fn random_key() -> String {
    use std::hash::{BuildHasher, Hasher};
    // RandomState is seeded from the OS's random source.
    let half = || std::collections::hash_map::RandomState::new().build_hasher().finish();
    format!("{:016x}{:016x}", half(), half())
}

/// FNV-1a, a stable hash for naming the socket.
#[cfg_attr(not(unix), allow(dead_code))]
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lychee-instance-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn requests_survive_the_wire() {
        let request = Request {
            open: vec!["/music/a b/01 – Ünïcode.flac".into(), "/music/folder".into()],
            edit: vec!["/music/edit me.mp3".into()],
        };
        assert_eq!(decode("k", &encode("k", &request)), Some(request.clone()));
        assert_eq!(decode("other", &encode("k", &request)), None, "a wrong key is refused");
        assert_eq!(decode("k", b"hello"), None);
        assert_eq!(decode("", &encode("", &Request::default())), Some(Request::default()));
    }

    #[test]
    fn a_second_launch_hands_its_files_to_the_first() {
        let dir = temp_dir("handover");
        let Ok(Claim::Primary(primary)) = claim(&dir, &Request::default()) else { panic!("the first launch runs") };
        let request = Request { open: vec!["/music/song.flac".into()], edit: Vec::new() };
        assert!(matches!(claim(&dir, &request), Ok(Claim::Forwarded)));
        assert_eq!(primary.requests.recv_timeout(Duration::from_secs(5)).ok(), Some(request));
        assert!(matches!(claim(&dir, &Request::default()), Ok(Claim::Forwarded)));
        assert_eq!(primary.requests.recv_timeout(Duration::from_secs(5)).ok(), Some(Request::default()));
        // Once the first one is gone, the next launch takes over.
        drop(primary);
        assert!(matches!(claim(&dir, &Request::default()), Ok(Claim::Primary(_))));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_launch_while_lychee_shuts_down_waits_and_takes_over() {
        let dir = temp_dir("closing");
        let Ok(Claim::Primary(primary)) = claim(&dir, &Request::default()) else { panic!("the first launch runs") };
        primary.close();
        let later = {
            let dir = dir.clone();
            std::thread::spawn(move || claim(&dir, &Request { open: vec!["/music/a.flac".into()], edit: Vec::new() }))
        };
        // The closing Lychee takes a moment to save, then goes.
        std::thread::sleep(Duration::from_millis(400));
        assert!(primary.requests.try_recv().is_err(), "nothing is handed to a closing Lychee");
        drop(primary);
        assert!(matches!(later.join().expect("no panic"), Ok(Claim::Primary(_))));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn separate_data_folders_are_separate_players() {
        let (a, b) = (temp_dir("one"), temp_dir("two"));
        let first = claim(&a, &Request::default());
        let second = claim(&b, &Request::default());
        assert!(matches!(first, Ok(Claim::Primary(_))));
        assert!(matches!(second, Ok(Claim::Primary(_))));
        drop((first, second));
        let _ = std::fs::remove_dir_all(&a);
        let _ = std::fs::remove_dir_all(&b);
    }
}
