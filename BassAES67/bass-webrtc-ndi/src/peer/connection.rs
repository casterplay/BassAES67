//! Single WebRTC peer connection.
//!
//! Wraps RTCPeerConnection from webrtc-rs and handles SDP/ICE exchange.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use ringbuf::traits::{Producer, Split};
use ringbuf::HeapRb;
use tokio::sync::mpsc;
use webrtc::api::API;
use webrtc::ice_transport::ice_candidate::RTCIceCandidate;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::rtp_transceiver::rtp_receiver::RTCRtpReceiver;
use webrtc::rtp_transceiver::RTCRtpTransceiver;
use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;
use webrtc::track::track_local::TrackLocal;
use webrtc::track::track_remote::TrackRemote;

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
    /// Ring buffer consumer for incoming audio (browser -> BASS)
    incoming_consumer: Option<ringbuf::HeapCons<f32>>,
    /// Connection state
    state: Arc<AtomicU32>,
    /// Statistics
    pub stats: Arc<PeerStats>,
    /// Channel to receive ICE candidates for signaling
    ice_candidate_rx: Option<mpsc::Receiver<IceCandidateInfo>>,
    /// Sender for ICE candidates
    ice_candidate_tx: mpsc::Sender<IceCandidateInfo>,
    /// Sample rate for decoder
    sample_rate: u32,
    /// Number of channels for decoder
    channels: u16,
}

impl WebRtcPeer {
    /// Create a new WebRTC peer connection.
    pub async fn new(
        id: u32,
        api: &API,
        config: RTCConfiguration,
        shared_track: Arc<TrackLocalStaticSample>,
        incoming_buffer_size: usize,
        sample_rate: u32,
        channels: u16,
    ) -> Result<Self, String> {
        let peer_connection = api
            .new_peer_connection(config)
            .await
            .map_err(|e| format!("Failed to create peer connection: {}", e))?;

        let peer_connection = Arc::new(peer_connection);

        let rb = HeapRb::<f32>::new(incoming_buffer_size);
        let (producer, consumer) = rb.split();

        let (ice_tx, ice_rx) = mpsc::channel::<IceCandidateInfo>(32);

        let state = Arc::new(AtomicU32::new(PEER_STATE_NEW));
        let stats = Arc::new(PeerStats::default());

        let _rtp_sender = peer_connection
            .add_track(shared_track.clone() as Arc<dyn TrackLocal + Send + Sync>)
            .await
            .map_err(|e| format!("Failed to add track: {}", e))?;

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

        let producer = Arc::new(parking_lot::Mutex::new(Some(producer)));
        let stats_for_track = stats.clone();
        let track_sample_rate = sample_rate;
        let track_channels = channels;

        peer_connection.on_track(Box::new(
            move |track: Arc<TrackRemote>, _receiver: Arc<RTCRtpReceiver>, _transceiver: Arc<RTCRtpTransceiver>| {
                let codec = track.codec();

                if !codec.capability.mime_type.to_lowercase().contains("opus") {
                    return Box::pin(async {});
                }

                let producer = match producer.lock().take() {
                    Some(p) => p,
                    None => {
                        eprintln!("WebRTC: on_track called but producer already taken");
                        return Box::pin(async {});
                    }
                };

                let stats = stats_for_track.clone();
                let sample_rate = track_sample_rate;
                let channels = track_channels;

                Box::pin(async move {
                    spawn_track_reader(track, producer, stats, sample_rate, channels).await;
                })
            },
        ));

        Ok(Self {
            id,
            peer_connection,
            outgoing_track: shared_track,
            incoming_consumer: Some(consumer),
            state,
            stats,
            ice_candidate_rx: Some(ice_rx),
            ice_candidate_tx: ice_tx,
            sample_rate,
            channels,
        })
    }

    /// Take the incoming audio consumer (for use by input stream)
    pub fn take_incoming_consumer(&mut self) -> Option<ringbuf::HeapCons<f32>> {
        self.incoming_consumer.take()
    }

    /// Take the ICE candidate receiver (for signaling)
    pub fn take_ice_candidate_rx(&mut self) -> Option<mpsc::Receiver<IceCandidateInfo>> {
        self.ice_candidate_rx.take()
    }

    /// Handle an SDP offer from the remote peer.
    pub async fn handle_offer(&self, offer: &str) -> Result<String, String> {
        let offer_sdp = RTCSessionDescription::offer(offer.to_string())
            .map_err(|e| format!("Invalid offer SDP: {}", e))?;

        self.peer_connection
            .set_remote_description(offer_sdp)
            .await
            .map_err(|e| format!("Failed to set remote description: {}", e))?;

        let answer = self
            .peer_connection
            .create_answer(None)
            .await
            .map_err(|e| format!("Failed to create answer: {}", e))?;

        self.peer_connection
            .set_local_description(answer.clone())
            .await
            .map_err(|e| format!("Failed to set local description: {}", e))?;

        Ok(answer.sdp)
    }

    /// Create an SDP offer (when we initiate the connection).
    pub async fn create_offer(&self) -> Result<String, String> {
        let offer = self
            .peer_connection
            .create_offer(None)
            .await
            .map_err(|e| format!("Failed to create offer: {}", e))?;

        self.peer_connection
            .set_local_description(offer.clone())
            .await
            .map_err(|e| format!("Failed to set local description: {}", e))?;

        Ok(offer.sdp)
    }

    /// Handle an SDP answer from the remote peer.
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

/// Spawn an async task to read RTP from a remote track and decode OPUS to PCM.
async fn spawn_track_reader(
    track: Arc<TrackRemote>,
    mut producer: ringbuf::HeapProd<f32>,
    stats: Arc<PeerStats>,
    sample_rate: u32,
    channels: u16,
) {
    let format = AudioFormat::new(sample_rate, channels as u8);
    let mut decoder = match OpusDecoder::new(format, 20.0) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("WebRTC: Failed to create OPUS decoder: {:?}", e);
            return;
        }
    };

    let max_samples = decoder.total_samples_per_frame();
    let mut pcm_buffer = vec![0.0f32; max_samples];

    loop {
        match track.read_rtp().await {
            Ok((rtp_packet, _attributes)) => {
                let payload = rtp_packet.payload.as_ref();
                if payload.is_empty() {
                    continue;
                }

                match decoder.decode_float(payload, &mut pcm_buffer, false) {
                    Ok(samples_per_channel) => {
                        let total_samples = samples_per_channel * channels as usize;
                        let pushed = producer.push_slice(&pcm_buffer[..total_samples]);

                        if pushed < total_samples {
                            stats.decode_errors.fetch_add(1, Ordering::Relaxed);
                        }

                        stats.packets_received.fetch_add(1, Ordering::Relaxed);
                        stats.bytes_received.fetch_add(payload.len() as u64, Ordering::Relaxed);
                    }
                    Err(e) => {
                        stats.decode_errors.fetch_add(1, Ordering::Relaxed);
                        eprintln!("WebRTC: OPUS decode error: {:?}", e);
                    }
                }
            }
            Err(e) => {
                let err_str = e.to_string().to_lowercase();
                if err_str.contains("eof") || err_str.contains("closed") || err_str.contains("data channel closed") {
                    break;
                }
                eprintln!("WebRTC: RTP read error: {}", e);
            }
        }
    }
}
