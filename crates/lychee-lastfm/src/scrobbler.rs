//! Background scrobbling with a persistent offline queue and exponential back-off.

use std::path::PathBuf;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};

use crate::{Client, Credentials, Scrobble, Session, TrackInfo};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum State {
    /// No account connected.
    #[default]
    Disconnected,
    /// Connected and up to date.
    Idle,
    /// Sending scrobbles right now.
    Submitting,
    /// Last.fm is unreachable; scrobbles are queued and retried automatically.
    Offline,
    /// The session was revoked or the API key is invalid; reconnect in Settings.
    AuthError,
}

#[derive(Clone, Debug, Default)]
pub struct Status {
    pub state: State,
    pub username: Option<String>,
    pub pending: usize,
    pub scrobbled_this_session: usize,
    pub last_scrobbled: Option<String>,
    pub last_error: Option<String>,
}

enum Msg {
    NowPlaying(TrackInfo),
    Scrobble(Scrobble),
    Love { artist: String, title: String, loved: bool },
    Configure { credentials: Option<Credentials>, session: Option<Session> },
    Retry,
    Quit,
}

pub struct Scrobbler {
    tx: Sender<Msg>,
    status: Arc<parking_lot_free::Mutex<Status>>,
    handle: Option<JoinHandle<()>>,
}

// std's Mutex is enough here; avoid an extra dependency.
mod parking_lot_free {
    pub type Mutex<T> = std::sync::Mutex<T>;
}

impl Scrobbler {
    /// Start the worker. `on_status` is called (from the worker thread) whenever the status changes.
    pub fn start(
        queue_file: PathBuf,
        credentials: Option<Credentials>,
        session: Option<Session>,
        on_status: impl Fn(Status) + Send + 'static,
    ) -> Scrobbler {
        let (tx, rx) = crossbeam_channel::unbounded();
        let status = Arc::new(std::sync::Mutex::new(Status::default()));
        let worker_status = status.clone();
        let handle = thread::Builder::new()
            .name("lychee-lastfm".into())
            .spawn(move || {
                let mut worker = Worker::new(queue_file, worker_status, Box::new(on_status));
                worker.configure(credentials, session);
                worker.run(rx);
            })
            .expect("failed to spawn the Last.fm thread");
        Scrobbler { tx, status, handle: Some(handle) }
    }

    pub fn now_playing(&self, track: TrackInfo) {
        let _ = self.tx.send(Msg::NowPlaying(track));
    }

    pub fn scrobble(&self, scrobble: Scrobble) {
        let _ = self.tx.send(Msg::Scrobble(scrobble));
    }

    pub fn set_loved(&self, artist: String, title: String, loved: bool) {
        let _ = self.tx.send(Msg::Love { artist, title, loved });
    }

    /// Change account. `None` session disconnects (queued scrobbles are kept).
    pub fn configure(&self, credentials: Option<Credentials>, session: Option<Session>) {
        let _ = self.tx.send(Msg::Configure { credentials, session });
    }

    pub fn retry(&self) {
        let _ = self.tx.send(Msg::Retry);
    }

    pub fn status(&self) -> Status {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

impl Drop for Scrobbler {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Quit);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

struct Worker {
    client: Option<Client>,
    session: Option<Session>,
    queue: Vec<Scrobble>,
    queue_file: PathBuf,
    status: Arc<std::sync::Mutex<Status>>,
    on_status: Box<dyn Fn(Status) + Send>,
    backoff: Duration,
    next_attempt: Option<Instant>,
}

const MIN_BACKOFF: Duration = Duration::from_secs(30);
const MAX_BACKOFF: Duration = Duration::from_secs(30 * 60);

impl Worker {
    fn new(queue_file: PathBuf, status: Arc<std::sync::Mutex<Status>>, on_status: Box<dyn Fn(Status) + Send>) -> Self {
        let queue = std::fs::read_to_string(&queue_file)
            .ok()
            .and_then(|s| serde_json::from_str::<Vec<Scrobble>>(&s).ok())
            .unwrap_or_default();
        Worker { client: None, session: None, queue, queue_file, status, on_status, backoff: MIN_BACKOFF, next_attempt: None }
    }

    fn update(&self, f: impl FnOnce(&mut Status)) {
        let snapshot = {
            let mut s = self.status.lock().expect("status lock");
            f(&mut s);
            s.pending = self.queue.len();
            s.clone()
        };
        (self.on_status)(snapshot);
    }

    fn configure(&mut self, credentials: Option<Credentials>, session: Option<Session>) {
        self.client = credentials.filter(Credentials::is_complete).map(Client::new);
        self.session = session;
        self.backoff = MIN_BACKOFF;
        self.next_attempt = None;
        let connected = self.client.is_some() && self.session.is_some();
        let username = self.session.as_ref().map(|s| s.username.clone());
        self.update(|s| {
            s.state = if connected { State::Idle } else { State::Disconnected };
            s.username = username;
            s.last_error = None;
        });
    }

    fn connected(&self) -> Option<(&Client, &Session)> {
        Some((self.client.as_ref()?, self.session.as_ref()?))
    }

    fn save_queue(&self) {
        let tmp = self.queue_file.with_extension("tmp");
        let ok = serde_json::to_string(&self.queue)
            .ok()
            .and_then(|json| std::fs::write(&tmp, json).ok())
            .and_then(|_| std::fs::rename(&tmp, &self.queue_file).ok());
        if ok.is_none() {
            tracing::warn!("could not save the scrobble queue to {}", self.queue_file.display());
        }
    }

    fn run(&mut self, rx: Receiver<Msg>) {
        loop {
            let wait = match self.next_attempt {
                Some(at) => at.saturating_duration_since(Instant::now()),
                None if !self.queue.is_empty() && self.connected().is_some() => Duration::ZERO,
                None => Duration::from_secs(3600),
            };
            match rx.recv_timeout(wait) {
                Ok(Msg::Quit) | Err(RecvTimeoutError::Disconnected) => return,
                Ok(msg) => self.handle(msg),
                Err(RecvTimeoutError::Timeout) => self.next_attempt = None,
            }
            if self.next_attempt.is_none() {
                self.submit();
            }
        }
    }

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::NowPlaying(track) => {
                if let Some((client, session)) = self.connected() {
                    if let Err(e) = client.update_now_playing(session, &track) {
                        tracing::debug!("now playing update failed: {e}");
                        if e.is_auth() {
                            self.update(|s| {
                                s.state = State::AuthError;
                                s.last_error = Some(e.to_string());
                            });
                        }
                    }
                }
            }
            Msg::Scrobble(scrobble) => {
                self.queue.push(scrobble);
                self.save_queue();
                self.update(|_| {});
            }
            Msg::Love { artist, title, loved } => {
                if let Some((client, session)) = self.connected() {
                    if let Err(e) = client.set_loved(session, &artist, &title, loved) {
                        tracing::warn!("could not update the loved state on Last.fm: {e}");
                    }
                }
            }
            Msg::Configure { credentials, session } => self.configure(credentials, session),
            Msg::Retry => {
                self.next_attempt = None;
                self.backoff = MIN_BACKOFF;
            }
            Msg::Quit => {}
        }
    }

    fn submit(&mut self) {
        while !self.queue.is_empty() {
            let Some((client, session)) = self.connected() else { return };
            if self.status.lock().map(|s| s.state == State::AuthError).unwrap_or(false) {
                return;
            }
            let batch: Vec<Scrobble> = self.queue.iter().take(50).cloned().collect();
            let result = client.scrobble(session, &batch);
            match result {
                Ok(accepted) => {
                    let last = batch.last().map(|s| format!("{} — {}", s.track.artist, s.track.title));
                    self.queue.drain(..batch.len());
                    self.save_queue();
                    self.backoff = MIN_BACKOFF;
                    self.update(|s| {
                        s.state = State::Idle;
                        s.scrobbled_this_session += accepted;
                        s.last_scrobbled = last;
                        s.last_error = None;
                    });
                }
                Err(e) if e.is_auth() => {
                    self.update(|s| {
                        s.state = State::AuthError;
                        s.last_error = Some(e.to_string());
                    });
                    return;
                }
                Err(e) if e.is_temporary() => {
                    self.next_attempt = Some(Instant::now() + self.backoff);
                    self.backoff = (self.backoff * 2).min(MAX_BACKOFF);
                    self.update(|s| {
                        s.state = State::Offline;
                        s.last_error = Some(e.to_string());
                    });
                    return;
                }
                Err(e) => {
                    // A malformed scrobble would block the queue forever: drop it and move on.
                    tracing::warn!("Last.fm rejected a scrobble, dropping it: {e}");
                    self.queue.remove(0);
                    self.save_queue();
                    self.update(|s| s.last_error = Some(e.to_string()));
                }
            }
        }
    }
}
