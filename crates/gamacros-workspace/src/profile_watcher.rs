use std::time::Duration;
use std::{fs, path::Path};
use std::sync::mpsc;

use log::{debug, warn};
use thiserror::Error;
use notify::{Config, Error as NotifyError, RecursiveMode};
use notify_debouncer_mini::{
    new_debouncer_opt, DebounceEventResult, DebouncedEventKind, Debouncer,
};

use crate::profile_parse::parse_profile;
use crate::profile::{ProfileError, Profile};

#[derive(Error, Debug)]
pub enum WatcherError {
    #[error("notify error: {0}")]
    Notify(#[from] NotifyError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse error: {0}")]
    Parse(#[from] ProfileError),
}

pub enum ProfileEvent {
    Changed(Profile),
    Removed,
    Error(WatcherError),
}

type ProfileEventSender = mpsc::Sender<ProfileEvent>;
pub type ProfileEventReceiver = mpsc::Receiver<ProfileEvent>;

fn send_profile_event(path: &Path, tx: &ProfileEventSender) {
    debug!("[watcher] reading profile from {}", path.display());
    match fs::read_to_string(path) {
        Ok(content) => match parse_profile(&content) {
            Ok(workspace) => {
                debug!("[watcher] profile parsed successfully, sending Changed event");
                let _ = tx.send(ProfileEvent::Changed(workspace));
            }
            Err(e) => {
                warn!("[watcher] profile parse error: {e}");
                let error = WatcherError::Parse(e);
                let _ = tx.send(ProfileEvent::Error(error));
            }
        },
        Err(e) => {
            warn!("[watcher] failed to read profile: {e}");
            let error = WatcherError::Io(e);
            let _ = tx.send(ProfileEvent::Error(error));
        }
    };
}

#[allow(dead_code)]
pub struct ProfileWatcher<W: notify::Watcher> {
    watcher: Debouncer<W>,
}

impl<W: notify::Watcher> ProfileWatcher<W> {
    pub fn new_with_sender(
        path: &Path,
        tx: ProfileEventSender,
    ) -> Result<Self, WatcherError> {
        // Resolve symlinks so the watcher monitors the real file's directory.
        // macOS FSEvents works at the directory level, so we watch the parent
        // directory and filter events by filename.
        let resolved = fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
        let watch_dir = resolved.parent().unwrap_or(&resolved).to_owned();
        let file_name = resolved.file_name().map(|f| f.to_owned());
        let path_c = resolved.clone();
        let tx_c = tx.clone();

        debug!(
            "[watcher] watching dir={} for file={} (resolved={})",
            watch_dir.display(),
            file_name.as_ref().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default(),
            resolved.display(),
        );

        let debouncer_config = notify_debouncer_mini::Config::default()
            .with_timeout(Duration::from_millis(500))
            .with_notify_config(Config::default());

        let mut debouncer = new_debouncer_opt::<_, W>(
            debouncer_config,
            move |events: DebounceEventResult| match events {
                Ok(events) => {
                    for event in events {
                        debug!(
                            "[watcher] fs event: path={} kind={:?}",
                            event.path.display(),
                            event.kind,
                        );
                        // Match by filename to handle editors that save via
                        // temp-file + rename (canonicalize may differ).
                        let matches = match (&file_name, event.path.file_name()) {
                            (Some(expected), Some(actual)) => expected == actual,
                            _ => {
                                // Fallback to full canonicalized path comparison
                                let event_path = fs::canonicalize(&event.path)
                                    .unwrap_or_else(|_| event.path.clone());
                                event_path == path_c
                            }
                        };
                        if !matches {
                            debug!(
                                "[watcher] skipping event (not our file): {}",
                                event.path.display(),
                            );
                            continue;
                        }
                        match event.kind {
                            DebouncedEventKind::Any
                            | DebouncedEventKind::AnyContinuous => {
                                if !path_c.exists() {
                                    debug!("[watcher] profile file removed");
                                    let _ = tx_c.send(ProfileEvent::Removed);
                                } else {
                                    debug!("[watcher] profile file changed, reloading");
                                    send_profile_event(&path_c, &tx_c);
                                }
                            }
                            _ => {
                                debug!("[watcher] ignoring event kind {:?}", event.kind);
                            }
                        }
                    }
                }
                Err(event) => {
                    warn!("[watcher] notify error: {event}");
                    let error = WatcherError::Notify(event);
                    let _ = tx_c.send(ProfileEvent::Error(error));
                }
            },
        )?;

        debouncer
            .watcher()
            .watch(&watch_dir, RecursiveMode::NonRecursive)?;
        debug!("[watcher] started successfully");

        Ok(Self { watcher: debouncer })
    }

    pub fn new(path: &Path) -> Result<(Self, ProfileEventReceiver), WatcherError> {
        let (tx, rx) = mpsc::channel();

        Ok((Self::new_with_sender(path, tx)?, rx))
    }

    pub fn new_with_starting_event(
        path: &Path,
    ) -> Result<(Self, ProfileEventReceiver), WatcherError> {
        let (tx, rx) = mpsc::channel();

        // Send initial workspace event
        send_profile_event(path, &tx);
        Ok((Self::new_with_sender(path, tx)?, rx))
    }
}
