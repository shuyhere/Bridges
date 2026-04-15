use std::collections::HashMap;
use std::net::SocketAddr;

use crate::coord_client::CoordClient;
use crate::mdns;
use crate::noise::{NoiseHandshake, NoiseSession};

/// Connection path to a peer.
#[derive(Debug, Clone)]
pub enum ConnPath {
    Lan(SocketAddr),
    Direct(SocketAddr),
    Derp,
}

/// State machine for peer connection attempts.
#[derive(Debug, Clone)]
pub enum ConnState {
    Idle,
    TryingLan,
    TryingDirect,
    Connected,
}

/// Noise session state for a peer.
#[allow(clippy::large_enum_variant)]
pub enum SessionState {
    /// No session established yet.
    None,
    /// Handshake in progress (waiting for response).
    HandshakePending(NoiseHandshake),
    /// Fully established Noise transport session.
    Established(NoiseSession),
}

/// Per-peer connection state.
/// Session keys and nonces are managed by snow inside NoiseSession.
pub struct PeerConn {
    pub state: ConnState,
    pub session: SessionState,
    /// Previous session kept briefly during rekey to decrypt in-flight packets.
    pub previous_session: Option<NoiseSession>,
}

impl PeerConn {
    pub fn new(_peer_id: &str) -> Self {
        Self {
            state: ConnState::Idle,
            session: SessionState::None,
            previous_session: None,
        }
    }
}

/// Manages connections to all known peers.
pub struct ConnManager {
    pub peers: HashMap<String, PeerConn>,
    pub coord: Option<CoordClient>,
}

impl ConnManager {
    pub fn new(coord: Option<CoordClient>) -> Self {
        Self {
            peers: HashMap::new(),
            coord,
        }
    }

    /// Get or create a PeerConn entry.
    pub fn get_or_create(&mut self, peer_id: &str) -> &mut PeerConn {
        self.peers
            .entry(peer_id.to_string())
            .or_insert_with(|| PeerConn::new(peer_id))
    }

    /// Try LAN -> direct -> DERP in order, returning the best path found.
    pub async fn connect(&mut self, peer_id: &str) -> Result<ConnPath, String> {
        self.get_or_create(peer_id);

        // 1. Try LAN via mDNS
        if let Some(pc) = self.peers.get_mut(peer_id) {
            pc.state = ConnState::TryingLan;
        }
        let lan_peers = mdns::discover().await;
        for (id, addr) in &lan_peers {
            if id == peer_id {
                let path = ConnPath::Lan(*addr);
                if let Some(pc) = self.peers.get_mut(peer_id) {
                    pc.state = ConnState::Connected;
                }
                return Ok(path);
            }
        }

        // 2. Try direct via coordination server endpoint hints
        if let Some(pc) = self.peers.get_mut(peer_id) {
            pc.state = ConnState::TryingDirect;
        }
        if let Some(coord) = &self.coord {
            if let Ok(endpoints) = coord.get_peer_endpoints(peer_id).await {
                for ep in &endpoints {
                    if let Ok(addr) = ep.addr.parse::<SocketAddr>() {
                        let path = ConnPath::Direct(addr);
                        if let Some(pc) = self.peers.get_mut(peer_id) {
                            pc.state = ConnState::Connected;
                        }
                        return Ok(path);
                    }
                }
            }
        }

        // 3. Fall back to DERP relay
        let path = ConnPath::Derp;
        if let Some(pc) = self.peers.get_mut(peer_id) {
            pc.state = ConnState::Connected;
        }
        Ok(path)
    }
}
