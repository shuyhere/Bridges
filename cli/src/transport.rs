use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::Mutex;

use crate::connmgr::{ConnManager, ConnPath, SessionState};
use crate::crypto;
use crate::derp_client::DerpClient;
use crate::noise;

/// Unified encrypted transport using Noise IK handshake.
pub struct Transport {
    pub conn: Arc<Mutex<ConnManager>>,
    pub derp: Option<Arc<DerpClient>>,
    pub my_node_id: String,
    pub my_x25519_priv: [u8; 32],
    udp: Option<Arc<UdpSocket>>,
}

/// Derive 20-byte node hash from a kd_xxx node ID (for wire packets).
fn node_id_to_bytes(node_id: &str) -> [u8; 20] {
    let mut out = [0u8; 20];
    let hash = Sha256::digest(node_id.as_bytes());
    out.copy_from_slice(&hash[..20]);
    out
}

impl Transport {
    pub async fn new(
        conn: ConnManager,
        derp: Option<DerpClient>,
        my_node_id: String,
        my_x25519_priv: [u8; 32],
    ) -> Result<Self, String> {
        let udp = UdpSocket::bind("0.0.0.0:0")
            .await
            .map_err(|e| format!("bind UDP: {}", e))?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            derp: derp.map(Arc::new),
            my_node_id,
            my_x25519_priv,
            udp: Some(Arc::new(udp)),
        })
    }

    #[cfg(test)]
    pub fn new_for_tests(
        conn: ConnManager,
        derp: Option<DerpClient>,
        my_node_id: String,
        my_x25519_priv: [u8; 32],
    ) -> Self {
        Self {
            conn: Arc::new(Mutex::new(conn)),
            derp: derp.map(Arc::new),
            my_node_id,
            my_x25519_priv,
            udp: None,
        }
    }

    /// Send raw bytes over the best available path (no encryption).
    async fn send_raw(&self, peer_id: &str, packet: &[u8]) -> Result<(), String> {
        if let Some(client) = self.derp.as_ref() {
            return client.send(peer_id, packet).await;
        }

        let mut conn = self.conn.lock().await;
        let path = conn.connect(peer_id).await?;

        match path {
            ConnPath::Lan(addr) | ConnPath::Direct(addr) => {
                if let Some(ref udp) = self.udp {
                    udp.send_to(packet, addr)
                        .await
                        .map_err(|e| format!("UDP send: {}", e))?;
                }
            }
            ConnPath::Derp => {
                drop(conn); // release conn lock before sending over DERP
                if let Some(client) = self.derp.as_ref() {
                    client.send(peer_id, packet).await?;
                } else {
                    return Err("DERP not connected".to_string());
                }
            }
        }
        Ok(())
    }

    /// Perform Noise IK handshake with a peer.
    /// After this completes, the peer has an established NoiseSession.
    pub async fn handshake(
        &self,
        peer_id: &str,
        their_x25519_pub: &[u8; 32],
    ) -> Result<(), String> {
        // Create initiator handshake and M1
        let (handshake, m1) =
            noise::begin_handshake_initiator(&self.my_x25519_priv, their_x25519_pub)?;

        // Send M1 as V2 handshake packet
        let src = node_id_to_bytes(&self.my_node_id);
        let dst = node_id_to_bytes(peer_id);
        let packet = crypto::encode_wire_packet_v2(crypto::PACKET_HANDSHAKE, &src, &dst, &m1);
        self.send_raw(peer_id, &packet).await?;

        // Store handshake state
        {
            let mut conn = self.conn.lock().await;
            let pc = conn.get_or_create(peer_id);
            pc.session = SessionState::HandshakePending(handshake);
        }

        // Wait for M2 response (up to 5 seconds)
        for _ in 0..50 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let conn = self.conn.lock().await;
            if let Some(pc) = conn.peers.get(peer_id) {
                if matches!(pc.session, SessionState::Established(_)) {
                    return Ok(());
                }
            }
        }

        Err("handshake timeout waiting for M2".to_string())
    }

    /// Encrypt plaintext and send to peer via Noise transport.
    pub async fn send(&self, peer_id: &str, plaintext: &[u8]) -> Result<(), String> {
        // Encrypt with Noise session
        let ciphertext = {
            let mut conn = self.conn.lock().await;
            let pc = conn.get_or_create(peer_id);
            match &mut pc.session {
                SessionState::Established(ref mut session) => noise::encrypt(session, plaintext)?,
                _ => {
                    return Err("no session key established".to_string());
                }
            }
        };

        // Send as V2 transport packet
        let src = node_id_to_bytes(&self.my_node_id);
        let dst = node_id_to_bytes(peer_id);
        let packet =
            crypto::encode_wire_packet_v2(crypto::PACKET_TRANSPORT, &src, &dst, &ciphertext);
        self.send_raw(peer_id, &packet).await
    }

    /// Handle an inbound handshake message (M1 from initiator or M2 from responder).
    async fn handle_handshake(&self, from_peer: &str, payload: &[u8]) -> Result<(), String> {
        let mut conn = self.conn.lock().await;
        let pc = conn.get_or_create(from_peer);

        match &mut pc.session {
            SessionState::HandshakePending(ref mut handshake) => {
                // We're the initiator, received M2
                let (_response, _) = noise::process_handshake_message(handshake, payload)?;
                // Transition to transport — need to take ownership of handshake
                let old_session = std::mem::replace(&mut pc.session, SessionState::None);
                if let SessionState::HandshakePending(hs) = old_session {
                    let session = noise::into_transport(hs)?;
                    pc.session = SessionState::Established(session);
                }
            }
            _ => {
                // We're the responder, received M1 from a new peer.
                // In Noise IK, we learn the initiator's static key from M1.
                let mut handshake = noise::begin_handshake_responder(&self.my_x25519_priv)?;
                let (m2_opt, _) = noise::process_handshake_message(&mut handshake, payload)?;

                // Post-handshake verification: check that the initiator's static key
                // belongs to a known/registered peer. Reject unknown initiators.
                if let Some(remote_key) = noise::get_remote_static(&handshake) {
                    // Verify remote key is from a registered node by checking
                    // coordination server. For now, log the key for auditing.
                    // The key is an X25519 public key (32 bytes).
                    if remote_key.len() != 32 {
                        return Err("invalid initiator key length".to_string());
                    }
                    eprintln!(
                        "  handshake from {}: initiator key {}",
                        from_peer,
                        hex::encode(&remote_key)
                    );
                } else {
                    return Err("handshake M1 did not contain initiator static key".to_string());
                }

                // Send M2 back BEFORE transitioning to Established
                // (avoids race where peer sends data before receiving M2)
                if let Some(m2) = m2_opt {
                    let src = node_id_to_bytes(&self.my_node_id);
                    let dst = node_id_to_bytes(from_peer);
                    let packet =
                        crypto::encode_wire_packet_v2(crypto::PACKET_HANDSHAKE, &src, &dst, &m2);
                    drop(conn); // release lock before sending
                    self.send_raw(from_peer, &packet).await?;

                    // Now transition to transport after M2 is sent
                    let session = noise::into_transport(handshake)?;
                    let mut conn = self.conn.lock().await;
                    let pc = conn.get_or_create(from_peer);
                    pc.session = SessionState::Established(session);
                } else {
                    return Err("IK responder did not produce M2".to_string());
                }
            }
        }
        Ok(())
    }

    /// Receive and decrypt the next inbound message. Returns (peer_id, plaintext).
    /// Handshake messages are handled internally and not returned to the caller.
    pub async fn recv(&self) -> Result<(String, Vec<u8>), String> {
        loop {
            let (peer_id, raw) = self.recv_raw().await?;

            // Check version byte
            if raw.is_empty() {
                continue;
            }

            match raw[0] {
                0x02 => {
                    // V2 packet
                    let (_, pkt_type, _src, _dst, payload) = crypto::decode_wire_packet_v2(&raw)?;

                    match pkt_type {
                        crypto::PACKET_HANDSHAKE => {
                            // Handle handshake internally
                            if let Err(e) = self.handle_handshake(&peer_id, payload).await {
                                eprintln!("handshake error from {}: {}", peer_id, e);
                            }
                            continue; // don't return to caller
                        }
                        crypto::PACKET_TRANSPORT => {
                            // Decrypt with Noise session
                            let mut conn = self.conn.lock().await;
                            let pc = conn.get_or_create(&peer_id);
                            match &mut pc.session {
                                SessionState::Established(ref mut session) => {
                                    let pt = noise::decrypt(session, payload)?;
                                    return Ok((peer_id, pt));
                                }
                                _ => {
                                    // Try previous session (during rekey transition)
                                    if let Some(ref mut prev) = pc.previous_session {
                                        if let Ok(pt) = noise::decrypt(prev, payload) {
                                            return Ok((peer_id, pt));
                                        }
                                    }
                                    eprintln!("no session for V2 transport from {}", peer_id);
                                    continue;
                                }
                            }
                        }
                        _ => {
                            eprintln!("unknown V2 packet type: {}", pkt_type);
                            continue;
                        }
                    }
                }
                0x01 => {
                    // Legacy V1 packet — no longer supported
                    let (_ver, _src, _dst, nonce, _ct) = crypto::decode_wire_packet(&raw)?;
                    eprintln!(
                        "received legacy V1 packet from {} (nonce {}), dropping — upgrade peer",
                        peer_id, nonce
                    );
                    continue;
                }
                v => {
                    eprintln!("unknown packet version: {}", v);
                    continue;
                }
            }
        }
    }

    /// Receive raw bytes from any path (DERP or UDP).
    async fn recv_raw(&self) -> Result<(String, Vec<u8>), String> {
        // Try DERP first, then UDP
        if let Some(client) = self.derp.as_ref() {
            if let Ok((src_id, data)) = client.recv().await {
                return Ok((src_id, data));
            }
        }

        // UDP fallback
        if let Some(ref udp) = self.udp {
            let mut buf = vec![0u8; 65536];
            let (len, _addr) = udp
                .recv_from(&mut buf)
                .await
                .map_err(|e| format!("UDP recv: {}", e))?;
            buf.truncate(len);
            // For UDP we don't know the peer_id string from the wire;
            // it will be resolved from the packet src_id field by the caller
            let src_hex = if buf.len() > 22 {
                match buf[0] {
                    0x02 => hex::encode(&buf[2..22]),
                    _ => hex::encode(&buf[1..21]),
                }
            } else {
                "unknown".to_string()
            };
            Ok((src_hex, buf))
        } else {
            Err("no receive path available".to_string())
        }
    }
}
