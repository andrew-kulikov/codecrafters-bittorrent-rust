use std::time::Duration;

use anyhow::bail;

use crate::log_error;
use crate::log_info;
use crate::peer::{HandshakeRequest, PeerConnection};
use crate::tracker::Peer;
use crate::utils::RawBytesExt;

/// How the session loop should proceed after handling an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionControl {
    /// Keep processing events on the current connection.
    Continue,
    /// Disconnect and retry the session with backoff.
    Reconnect,
    /// Stop the session altogether (worker completed or shutting down).
    Stop,
}

/// Callback interface for driving a peer session.
pub trait PeerSessionHandler {
    /// Called once after a connection is established and handshake succeeded.
    /// Return `Reconnect` to drop the connection immediately, or `Stop` to exit.
    fn on_connect(&mut self, _conn: &PeerConnection) -> anyhow::Result<SessionControl> {
        Ok(SessionControl::Continue)
    }

    /// Handle an incoming event. Use `SessionControl` to influence the loop.
    fn on_event(
        &mut self,
        conn: &PeerConnection,
        event: crate::peer::PeerEvent,
    ) -> anyhow::Result<SessionControl>;

    /// Let the handler signal shutdown (e.g., queue finished).
    fn should_stop(&self) -> bool {
        false
    }
}

/// Configuration for peer session retries.
#[derive(Clone, Debug)]
pub struct PeerSessionConfig {
    pub backoff_base_secs: f32,
    pub backoff_cap_secs: f32,
    pub max_retries: u8,
}

impl PeerSessionConfig {
    pub fn default() -> Self {
        // Using more aggressive backoff for faster testing cycles.
        Self {
            backoff_base_secs: 1.0,
            backoff_cap_secs: 3.0,
            max_retries: 2,
        }
    }

    pub fn aggressive() -> Self {
        Self {
            backoff_base_secs: 0.5,
            backoff_cap_secs: 1.0,
            max_retries: 1,
        }
    }
}

/// Thin orchestration layer that owns the reconnect/backoff loop and establishes
/// a `PeerConnection`, leaving event handling to a `PeerSessionHandler`.
pub struct PeerSession {
    peer: Peer,
    info_hash: Vec<u8>,
    client_id: String,
    config: PeerSessionConfig,
}

impl PeerSession {
    pub fn new(
        peer: Peer,
        info_hash: Vec<u8>,
        client_id: String,
        config: PeerSessionConfig,
    ) -> Self {
        Self {
            peer,
            info_hash,
            client_id,
            config,
        }
    }

    pub fn run<H: PeerSessionHandler>(&self, handler: &mut H) -> anyhow::Result<()> {
        let mut attempts = 0u32;

        while !handler.should_stop() {
            if self.config.max_retries > 0 && attempts >= self.config.max_retries as u32 {
                log_info!(
                    "PeerSession",
                    "[{}] Reached max retries ({}), stopping session",
                    self.peer,
                    self.config.max_retries
                );
                bail!("Max retries reached for peer {}", self.peer)
            }

            log_info!(
                "PeerSession",
                "[{}] Connecting (attempt {})",
                self.peer,
                attempts + 1
            );

            let handshake_req = HandshakeRequest::new_with_extension_support(
                self.info_hash.clone(),
                self.client_id.to_raw_bytes(),
            );

            let connection = match PeerConnection::new(self.peer.clone(), &handshake_req) {
                Ok(conn) => {
                    conn
                }
                Err(e) => {
                    log_error!(
                        "PeerSession",
                        "[{}] Failed to connect: {}",
                        self.peer,
                        e
                    );
                    attempts += 1;
                    std::thread::sleep(self.backoff_delay(attempts));
                    continue;
                }
            };

            match handler.on_connect(&connection)? {
                SessionControl::Stop => return Ok(()),
                SessionControl::Reconnect => {
                    attempts += 1;
                    std::thread::sleep(self.backoff_delay(attempts));
                    continue;
                }
                SessionControl::Continue => {}
            }

            let mut reconnect = false;
            while !handler.should_stop() {
                match connection.next_event() {
                    Some(event) => match handler.on_event(&connection, event)? {
                        SessionControl::Continue => {}
                        SessionControl::Reconnect => {
                            reconnect = true;
                            break;
                        }
                        SessionControl::Stop => return Ok(()),
                    },
                    None => {
                        reconnect = true;
                        break;
                    }
                }
            }

            if handler.should_stop() {
                return Ok(());
            }

            attempts += 1;
            std::thread::sleep(self.backoff_delay(attempts));

            if !reconnect {
                // We broke out without requesting reconnect; treat as stop.
                return Ok(());
            }
        }

        Ok(())
    }

    fn backoff_delay(&self, attempts: u32) -> Duration {
        let base = Duration::from_secs_f32(self.config.backoff_base_secs);
        let cap = Duration::from_secs_f32(
            self.config
                .backoff_cap_secs
                .max(self.config.backoff_base_secs),
        );
        let shift = attempts.min(10);
        let factor = 1u32 << shift;
        let wait = base.saturating_mul(factor);
        if wait > cap {
            cap
        } else {
            wait
        }
    }
}
