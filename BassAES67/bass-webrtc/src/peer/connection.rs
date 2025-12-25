//! Single WebRTC peer connection.
//!
//! Wraps RTCPeerConnection from webrtc-rs and handles SDP/ICE exchange.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use bytes::Bytes;
use ringbuf::traits::{Producer, Split};
use ringbuf::HeapRb;
use tokio::sync::mpsc;
use webrtc::api::media_engine::MIME_TYPE_OPUS;
use webrtc::api::API;
use webrtc::ice_transport::ice_candidate::RTCIceCandidate;
use webrtc::ice_transport::ice_connection_state::RTCIceConnectionState;
use webrtc::media::Sample;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;
use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;
use webrtc::track::track_local::TrackLocal;

use crate::codec::opus::Decoder as OpusDecoder;
use crate::codec::AudioFormat;

// Peer connection states
pub const PEER_STATE_NEW: u32 = 0;
pub const PEER_STATE_CONNECTING: u32 = 1;
pub const PEER_STATE_CONNECTED: u32 = 2;
pub const PEER_STATE_DISCONNECTED: u32 = 3;
pub const PEER_STATE_FAILED: u32 = 4;
pub const PEER_STATE_CLOSED: u32 = 5;

/// Per-peer statistics (atomic, lock-free)
pub struct PeerStats {
    pub packets_sent: AtomicU64,
    pub packets_received: AtomicU64,
    pub bytes_sent: AtomicU64,
    pub bytes_received: AtomicU64,
    pub encode_errors: AtomicU64,
    pub decode_errors: AtomicU64,
}

impl Default for PeerStats {
    fn default() -> Self {
        Self {
            packets_sent: AtomicU64::new(0),
            packets_received: AtomicU64::new(0),
            bytes_sent: AtomicU64::new(0),
            bytes_received: AtomicU64::new(0),
            encode_errors: AtomicU64::new(0),
            decode_errors: AtomicU64::new(0),
        }
    }
}

/// ICE candidate for signaling
#[derive(Clone, Debug)]
pub struct IceCandidateInfo {
    pub candidate: String,
    pub sdp_mid: Option<String>,
    pub sdp_mline_index: Option<u16>,
}

/// Represents a single WebRTC peer connection
pub struct WebRtcPeer {
    /// Unique peer identifier (0-4)
    pub id: u32,
    /// The RTCPeerConnection instance
    peer_connection: Arc<RTCPeerConnection>,
    /// Outgoing audio track (BASS -> browser)
    outgoing_track: Arc<TrackLocalStaticSample>,
    /// Ring buffer producer for incoming audio (browser -> BASS)
    incoming_producer: Option<ringbuf::HeapProd<f32>>,
    /// Connection state
    state: Arc<AtomicU32>,
    /// Statistics
    pub stats: Arc<PeerStats>,
    /// Channel to receive ICE candidates for signaling
    ice_candidate_rx: Option<mpsc::Receiver<IceCandidateInfo>>,
    /// Sender for ICE candidates (kept alive while connection exists)
    ice_candidate_tx: mpsc::Sender<IceCandidateInfo>,
}

impl WebRtcPeer {
    /// Create a new WebRTC peer connection.
    ///
    /// # Arguments
    /// * `id` - Unique peer identifier (0-4)
    /// * `api` - Shared WebRTC API instance
    /// * `config` - RTCConfiguration with ICE servers
    /// * `shared_track` - Shared outgoing audio track
    /// * `incoming_buffer_size` - Size of incoming audio ring buffer in samples
    pub async fn new(
        id: u32,
        api: &API,
        config: RTCConfiguration,
        shared_track: Arc<TrackLocalStaticSample>,
        incoming_buffer_size: usize,
    ) -> Result<Self, String> {
        // Create peer connection
        let peer_connection = api
            .new_peer_connection(config)
            .await
            .map_err(|e| format!("Failed to create peer connection: {}", e))?;

        let peer_connection = Arc::new(peer_connection);

        // Create ring buffer for incoming audio
        let rb = HeapRb::<f32>::new(incoming_buffer_size);
        let (producer, _consumer) = rb.split();

        // Create channel for ICE candidates
        let (ice_tx, ice_rx) = mpsc::channel::<IceCandidateInfo>(32);

        // State tracking
        let state = Arc::new(AtomicU32::new(PEER_STATE_NEW));

        // Add the shared outgoing track
        let _rtp_sender = peer_connection
            .add_track(shared_track.clone() as Arc<dyn TrackLocal + Send + Sync>)
            .await
            .map_err(|e| format!("Failed to add track: {}", e))?;

        // Setup ICE candidate handler
        let ice_tx_clone = ice_tx.clone();
        peer_connection.on_ice_candidate(Box::new(move |candidate: Option<RTCIceCandidate>| {
            let ice_tx = ice_tx_clone.clone();
            Box::pin(async move {
                if let Some(c) = candidate {
                    let _ = ice_tx
                        .send(IceCandidateInfo {
                            candidate: c.to_json().map(|j| j.candidate).unwrap_or_default(),
                            sdp_mid: c.to_json().ok().and_then(|j| j.sdp_mid),
                            sdp_mline_index: c.to_json().ok().and_then(|j| j.sdp_mline_index),
                        })
                        .await;
                }
            })
        }));

        // Setup connection state handler
        let state_clone = state.clone();
        peer_connection.on_peer_connection_state_change(Box::new(move |s: RTCPeerConnectionState| {
            let new_state = match s {
                RTCPeerConnectionState::New => PEER_STATE_NEW,
                RTCPeerConnectionState::Connecting => PEER_STATE_CONNECTING,
                RTCPeerConnectionState::Connected => PEER_STATE_CONNECTED,
                RTCPeerConnectionState::Disconnected => PEER_STATE_DISCONNECTED,
                RTCPeerConnectionState::Failed => PEER_STATE_FAILED,
                RTCPeerConnectionState::Closed => PEER_STATE_CLOSED,
                _ => PEER_STATE_NEW,
            };
            state_clone.store(new_state, Ordering::SeqCst);
            Box::pin(async {})
        }));

        Ok(Self {
            id,
            peer_connection,
            outgoing_track: shared_track,
            incoming_producer: Some(producer),
            state,
            stats: Arc::new(PeerStats::default()),
            ice_candidate_rx: Some(ice_rx),
            ice_candidate_tx: ice_tx,
        })
    }

    /// Take the incoming audio producer (for use by input stream)
    pub fn take_incoming_producer(&mut self) -> Option<ringbuf::HeapProd<f32>> {
        self.incoming_producer.take()
    }

    /// Take the ICE candidate receiver (for signaling)
    pub fn take_ice_candidate_rx(&mut self) -> Option<mpsc::Receiver<IceCandidateInfo>> {
        self.ice_candidate_rx.take()
    }

    /// Handle an SDP offer from the remote peer.
    ///
    /// # Arguments
    /// * `offer` - SDP offer string
    ///
    /// # Returns
    /// SDP answer string
    pub async fn handle_offer(&self, offer: &str) -> Result<String, String> {
        // Parse offer
        let offer_sdp = RTCSessionDescription::offer(offer.to_string())
            .map_err(|e| format!("Invalid offer SDP: {}", e))?;

        // Set remote description
        self.peer_connection
            .set_remote_description(offer_sdp)
            .await
            .map_err(|e| format!("Failed to set remote description: {}", e))?;

        // Create answer
        let answer = self
            .peer_connection
            .create_answer(None)
            .await
            .map_err(|e| format!("Failed to create answer: {}", e))?;

        // Set local description
        self.peer_connection
            .set_local_description(answer.clone())
            .await
            .map_err(|e| format!("Failed to set local description: {}", e))?;

        Ok(answer.sdp)
    }

    /// Create an SDP offer (when we initiate the connection).
    ///
    /// # Returns
    /// SDP offer string
    pub async fn create_offer(&self) -> Result<String, String> {
        // Create offer
        let offer = self
            .peer_connection
            .create_offer(None)
            .await
            .map_err(|e| format!("Failed to create offer: {}", e))?;

        // Set local description
        self.peer_connection
            .set_local_description(offer.clone())
            .await
            .map_err(|e| format!("Failed to set local description: {}", e))?;

        Ok(offer.sdp)
    }

    /// Handle an SDP answer from the remote peer.
    ///
    /// # Arguments
    /// * `answer` - SDP answer string
    pub async fn handle_answer(&self, answer: &str) -> Result<(), String> {
        let answer_sdp = RTCSessionDescription::answer(answer.to_string())
            .map_err(|e| format!("Invalid answer SDP: {}", e))?;

        self.peer_connection
            .set_remote_description(answer_sdp)
            .await
            .map_err(|e| format!("Failed to set remote description: {}", e))?;

        Ok(())
    }

    /// Add an ICE candidate from the remote peer.
    ///
    /// # Arguments
    /// * `candidate` - ICE candidate string
    /// * `sdp_mid` - SDP media ID (optional)
    /// * `sdp_mline_index` - SDP media line index (optional)
    pub async fn add_ice_candidate(
        &self,
        candidate: &str,
        sdp_mid: Option<&str>,
        sdp_mline_index: Option<u16>,
    ) -> Result<(), String> {
        use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;

        let candidate_init = RTCIceCandidateInit {
            candidate: candidate.to_string(),
            sdp_mid: sdp_mid.map(|s| s.to_string()),
            sdp_mline_index,
            username_fragment: None,
        };

        self.peer_connection
            .add_ice_candidate(candidate_init)
            .await
            .map_err(|e| format!("Failed to add ICE candidate: {}", e))?;

        Ok(())
    }

    /// Get current connection state
    pub fn state(&self) -> u32 {
        self.state.load(Ordering::SeqCst)
    }

    /// Check if peer is connected
    pub fn is_connected(&self) -> bool {
        self.state() == PEER_STATE_CONNECTED
    }

    /// Close the peer connection
    pub async fn close(&self) -> Result<(), String> {
        self.peer_connection
            .close()
            .await
            .map_err(|e| format!("Failed to close peer connection: {}", e))?;
        Ok(())
    }

    /// Get the peer connection for advanced usage
    pub fn peer_connection(&self) -> &Arc<RTCPeerConnection> {
        &self.peer_connection
    }

    /// Get the outgoing audio track
    pub fn outgoing_track(&self) -> &Arc<TrackLocalStaticSample> {
        &self.outgoing_track
    }
}
