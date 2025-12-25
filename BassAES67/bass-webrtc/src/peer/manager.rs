//! Peer manager for handling up to 5 simultaneous WebRTC connections.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::{MediaEngine, MIME_TYPE_OPUS};
use webrtc::api::APIBuilder;
use webrtc::api::API;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;
use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;

use super::connection::{WebRtcPeer, IceCandidateInfo, PeerStats};

/// Maximum number of simultaneous peers
pub const MAX_PEERS: usize = 5;

/// ICE server configuration
#[derive(Clone, Debug)]
pub struct IceServerConfig {
    pub urls: Vec<String>,
    pub username: Option<String>,
    pub credential: Option<String>,
}

impl IceServerConfig {
    /// Create a STUN-only server config
    pub fn stun(url: &str) -> Self {
        Self {
            urls: vec![url.to_string()],
            username: None,
            credential: None,
        }
    }

    /// Create a TURN server config with credentials
    pub fn turn(url: &str, username: &str, credential: &str) -> Self {
        Self {
            urls: vec![url.to_string()],
            username: Some(username.to_string()),
            credential: Some(credential.to_string()),
        }
    }

    /// Convert to webrtc-rs RTCIceServer
    fn to_rtc_ice_server(&self) -> RTCIceServer {
        RTCIceServer {
            urls: self.urls.clone(),
            username: self.username.clone().unwrap_or_default(),
            credential: self.credential.clone().unwrap_or_default(),
            ..Default::default()
        }
    }
}

/// Manages up to 5 simultaneous WebRTC peers
pub struct PeerManager {
    /// WebRTC API instance (shared across all peers)
    api: API,
    /// ICE servers configuration
    ice_servers: Vec<IceServerConfig>,
    /// Shared outgoing audio track (BASS -> all browsers)
    shared_track: Arc<TrackLocalStaticSample>,
    /// Peer slots (fixed size array)
    peers: [Option<WebRtcPeer>; MAX_PEERS],
    /// Number of active peers
    active_count: AtomicU32,
    /// Incoming audio buffer size per peer (samples)
    incoming_buffer_size: usize,
}

impl PeerManager {
    /// Create a new peer manager.
    ///
    /// # Arguments
    /// * `ice_servers` - List of ICE (STUN/TURN) servers
    /// * `incoming_buffer_size` - Size of incoming audio ring buffer per peer (in samples)
    pub fn new(ice_servers: Vec<IceServerConfig>, incoming_buffer_size: usize) -> Result<Self, String> {
        // Create media engine and register OPUS codec
        let mut media_engine = MediaEngine::default();
        media_engine
            .register_default_codecs()
            .map_err(|e| format!("Failed to register codecs: {}", e))?;

        // Create interceptor registry
        let mut registry = Registry::new();
        registry = register_default_interceptors(registry, &mut media_engine)
            .map_err(|e| format!("Failed to register interceptors: {}", e))?;

        // Build API
        let api = APIBuilder::new()
            .with_media_engine(media_engine)
            .with_interceptor_registry(registry)
            .build();

        // Create shared outgoing audio track (48kHz stereo OPUS)
        let shared_track = Arc::new(TrackLocalStaticSample::new(
            RTCRtpCodecCapability {
                mime_type: MIME_TYPE_OPUS.to_owned(),
                clock_rate: 48000,
                channels: 2,
                sdp_fmtp_line: "minptime=10;useinbandfec=1".to_owned(),
                rtcp_feedback: vec![],
            },
            "audio".to_owned(),
            "bass-webrtc".to_owned(),
        ));

        Ok(Self {
            api,
            ice_servers,
            shared_track,
            peers: Default::default(),
            active_count: AtomicU32::new(0),
            incoming_buffer_size,
        })
    }

    /// Get the shared outgoing audio track
    pub fn shared_track(&self) -> &Arc<TrackLocalStaticSample> {
        &self.shared_track
    }

    /// Find first empty peer slot
    fn find_empty_slot(&self) -> Option<usize> {
        for i in 0..MAX_PEERS {
            if self.peers[i].is_none() {
                return Some(i);
            }
        }
        None
    }

    /// Build RTCConfiguration from ICE servers
    fn build_rtc_config(&self) -> RTCConfiguration {
        RTCConfiguration {
            ice_servers: self
                .ice_servers
                .iter()
                .map(|s| s.to_rtc_ice_server())
                .collect(),
            ..Default::default()
        }
    }

    /// Add a new peer with an SDP offer.
    ///
    /// # Arguments
    /// * `offer_sdp` - SDP offer from the remote peer
    ///
    /// # Returns
    /// Tuple of (peer_id, answer_sdp) on success
    pub async fn add_peer_with_offer(&mut self, offer_sdp: &str) -> Result<(u32, String), String> {
        let slot = self
            .find_empty_slot()
            .ok_or_else(|| "Maximum peers reached (5)".to_string())?;

        let config = self.build_rtc_config();

        let mut peer = WebRtcPeer::new(
            slot as u32,
            &self.api,
            config,
            self.shared_track.clone(),
            self.incoming_buffer_size,
        )
        .await?;

        // Handle the offer and get answer
        let answer = peer.handle_offer(offer_sdp).await?;

        self.peers[slot] = Some(peer);
        self.active_count.fetch_add(1, Ordering::SeqCst);

        Ok((slot as u32, answer))
    }

    /// Add a new peer that we initiate (create offer).
    ///
    /// # Returns
    /// Tuple of (peer_id, offer_sdp) on success
    pub async fn add_peer_with_new_offer(&mut self) -> Result<(u32, String), String> {
        let slot = self
            .find_empty_slot()
            .ok_or_else(|| "Maximum peers reached (5)".to_string())?;

        let config = self.build_rtc_config();

        let peer = WebRtcPeer::new(
            slot as u32,
            &self.api,
            config,
            self.shared_track.clone(),
            self.incoming_buffer_size,
        )
        .await?;

        // Create offer
        let offer = peer.create_offer().await?;

        self.peers[slot] = Some(peer);
        self.active_count.fetch_add(1, Ordering::SeqCst);

        Ok((slot as u32, offer))
    }

    /// Set the answer for a peer (when we initiated with an offer)
    pub async fn set_peer_answer(&mut self, peer_id: u32, answer_sdp: &str) -> Result<(), String> {
        let peer = self
            .peers
            .get_mut(peer_id as usize)
            .ok_or_else(|| "Invalid peer ID".to_string())?
            .as_ref()
            .ok_or_else(|| "Peer not found".to_string())?;

        peer.handle_answer(answer_sdp).await
    }

    /// Add ICE candidate to a peer
    pub async fn add_ice_candidate(
        &self,
        peer_id: u32,
        candidate: &str,
        sdp_mid: Option<&str>,
        sdp_mline_index: Option<u16>,
    ) -> Result<(), String> {
        let peer = self
            .peers
            .get(peer_id as usize)
            .ok_or_else(|| "Invalid peer ID".to_string())?
            .as_ref()
            .ok_or_else(|| "Peer not found".to_string())?;

        peer.add_ice_candidate(candidate, sdp_mid, sdp_mline_index).await
    }

    /// Remove a peer by ID
    pub async fn remove_peer(&mut self, peer_id: u32) -> Result<(), String> {
        let slot = peer_id as usize;
        if slot >= MAX_PEERS {
            return Err("Invalid peer ID".to_string());
        }

        if let Some(peer) = self.peers[slot].take() {
            peer.close().await?;
            self.active_count.fetch_sub(1, Ordering::SeqCst);
        }

        Ok(())
    }

    /// Get active peer count
    pub fn active_count(&self) -> u32 {
        self.active_count.load(Ordering::SeqCst)
    }

    /// Get a peer by ID
    pub fn get_peer(&self, peer_id: u32) -> Option<&WebRtcPeer> {
        self.peers.get(peer_id as usize).and_then(|p| p.as_ref())
    }

    /// Get a mutable peer by ID
    pub fn get_peer_mut(&mut self, peer_id: u32) -> Option<&mut WebRtcPeer> {
        self.peers.get_mut(peer_id as usize).and_then(|p| p.as_mut())
    }

    /// Iterate over all active peers
    pub fn for_each_peer<F: FnMut(u32, &WebRtcPeer)>(&self, mut f: F) {
        for (i, peer_opt) in self.peers.iter().enumerate() {
            if let Some(peer) = peer_opt {
                f(i as u32, peer);
            }
        }
    }

    /// Get aggregated statistics from all peers
    pub fn aggregate_stats(&self) -> AggregateStats {
        let mut stats = AggregateStats::default();
        stats.active_peers = self.active_count();

        for peer_opt in &self.peers {
            if let Some(peer) = peer_opt {
                let ps = &peer.stats;
                stats.total_packets_sent += ps.packets_sent.load(Ordering::Relaxed);
                stats.total_packets_received += ps.packets_received.load(Ordering::Relaxed);
                stats.total_bytes_sent += ps.bytes_sent.load(Ordering::Relaxed);
                stats.total_bytes_received += ps.bytes_received.load(Ordering::Relaxed);
                stats.total_encode_errors += ps.encode_errors.load(Ordering::Relaxed);
                stats.total_decode_errors += ps.decode_errors.load(Ordering::Relaxed);
            }
        }

        stats
    }

    /// Close all peer connections
    pub async fn close_all(&mut self) {
        for i in 0..MAX_PEERS {
            if let Some(peer) = self.peers[i].take() {
                let _ = peer.close().await;
            }
        }
        self.active_count.store(0, Ordering::SeqCst);
    }
}

/// Aggregated statistics from all peers
#[derive(Default, Debug, Clone)]
pub struct AggregateStats {
    pub active_peers: u32,
    pub total_packets_sent: u64,
    pub total_packets_received: u64,
    pub total_bytes_sent: u64,
    pub total_bytes_received: u64,
    pub total_encode_errors: u64,
    pub total_decode_errors: u64,
}
