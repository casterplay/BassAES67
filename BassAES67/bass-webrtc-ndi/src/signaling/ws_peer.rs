//! WebSocket-based WebRTC Peer
//!
//! A WebRTC peer that uses a WebSocket signaling server for SDP/ICE exchange.
//! Unlike WHIP/WHEP, this supports true bidirectional audio with a single
//! peer connection (sendrecv) and DataChannel support.
//!
//! Flow:
//! 1. Connect to WebSocket signaling server
//! 2. Wait for offer from browser (or create offer if initiator)
//! 3. Exchange SDP offer/answer via WebSocket
//! 4. Exchange ICE candidates via WebSocket
//! 5. Once connected, bidirectional audio flows via RTP

use std::ffi::{c_char, c_void, CString};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex as ParkingMutex;
use ringbuf::traits::{Producer, Split};
use ringbuf::HeapRb;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::data_channel::RTCDataChannel;
use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;
use webrtc::rtp_transceiver::rtp_receiver::RTCRtpReceiver;
use webrtc::rtp_transceiver::rtp_transceiver_direction::RTCRtpTransceiverDirection;
use webrtc::rtp_transceiver::RTCRtpTransceiver;
use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;
use webrtc::track::track_local::TrackLocal;
use webrtc::track::track_remote::TrackRemote;

use crate::codec::opus::Decoder as OpusDecoder;
use crate::codec::AudioFormat;
use crate::peer::IceServerConfig;

/// Signaling message types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SignalingMessage {
    /// SDP offer from browser
    Offer { sdp: String },
    /// SDP answer from this peer
    Answer { sdp: String },
    /// ICE candidate
    Ice {
        candidate: String,
        #[serde(rename = "sdpMLineIndex")]
        sdp_m_line_index: Option<u16>,
        #[serde(rename = "sdpMid")]
        sdp_mid: Option<String>,
    },
    /// DataChannel message (relayed through signaling for pre-connection)
    Data { channel: String, payload: String },
}

/// WebRTC peer statistics
pub struct WebRtcPeerStats {
    // Basic counters
    pub packets_sent: AtomicU64,
    pub packets_received: AtomicU64,
    pub bytes_sent: AtomicU64,
    pub bytes_received: AtomicU64,

    // From RemoteInboundRTPStats - network quality metrics
    pub round_trip_time_ms: AtomicU32,  // RTT in milliseconds
    pub packets_lost: AtomicI64,         // Can be negative (duplicates)
    pub fraction_lost: AtomicU32,        // Loss percent * 100 (e.g., 250 = 2.5%)
    pub jitter_ms: AtomicU32,            // Jitter in milliseconds

    // Connection quality indicator
    pub nack_count: AtomicU64,
}

impl Default for WebRtcPeerStats {
    fn default() -> Self {
        Self {
            packets_sent: AtomicU64::new(0),
            packets_received: AtomicU64::new(0),
            bytes_sent: AtomicU64::new(0),
            bytes_received: AtomicU64::new(0),
            round_trip_time_ms: AtomicU32::new(0),
            packets_lost: AtomicI64::new(0),
            fraction_lost: AtomicU32::new(0),
            jitter_ms: AtomicU32::new(0),
            nack_count: AtomicU64::new(0),
        }
    }
}

/// FFI-safe stats snapshot for callbacks
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct WebRtcPeerStatsSnapshot {
    pub packets_sent: u64,
    pub packets_received: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub round_trip_time_ms: u32,
    pub packets_lost: i64,
    pub fraction_lost_percent: f32,  // 0.0 - 100.0
    pub jitter_ms: u32,
    pub nack_count: u64,
}

impl WebRtcPeerStats {
    /// Create a snapshot for FFI callback
    pub fn to_snapshot(&self) -> WebRtcPeerStatsSnapshot {
        WebRtcPeerStatsSnapshot {
            packets_sent: self.packets_sent.load(Ordering::Relaxed),
            packets_received: self.packets_received.load(Ordering::Relaxed),
            bytes_sent: self.bytes_sent.load(Ordering::Relaxed),
            bytes_received: self.bytes_received.load(Ordering::Relaxed),
            round_trip_time_ms: self.round_trip_time_ms.load(Ordering::Relaxed),
            packets_lost: self.packets_lost.load(Ordering::Relaxed),
            fraction_lost_percent: self.fraction_lost.load(Ordering::Relaxed) as f32 / 100.0,
            jitter_ms: self.jitter_ms.load(Ordering::Relaxed),
            nack_count: self.nack_count.load(Ordering::Relaxed),
        }
    }
}

/// Callback for DataChannel messages
pub type DataChannelCallback = Arc<dyn Fn(&str, &[u8]) + Send + Sync>;

/// FFI callback types for peer events (matches lib.rs)
pub type OnConnectedCallback = unsafe extern "C" fn(user: *mut c_void);
pub type OnDisconnectedCallback = unsafe extern "C" fn(user: *mut c_void);
pub type OnErrorCallback = unsafe extern "C" fn(error_code: u32, error_msg: *const c_char, user: *mut c_void);
pub type OnStatsCallback = unsafe extern "C" fn(stats: *const WebRtcPeerStatsSnapshot, user: *mut c_void);

/// Holds FFI callbacks for peer state changes
#[derive(Clone)]
pub struct PeerCallbacks {
    pub on_connected: Option<OnConnectedCallback>,
    pub on_disconnected: Option<OnDisconnectedCallback>,
    pub on_error: Option<OnErrorCallback>,
    pub user: *mut c_void,
    /// Track if disconnected has already been fired (to prevent double-fire on Disconnected + Closed)
    disconnected_fired: Arc<AtomicBool>,
}

// SAFETY: The user pointer is passed by C# and remains valid for the lifetime of the peer
unsafe impl Send for PeerCallbacks {}
unsafe impl Sync for PeerCallbacks {}

impl Default for PeerCallbacks {
    fn default() -> Self {
        Self {
            on_connected: None,
            on_disconnected: None,
            on_error: None,
            user: std::ptr::null_mut(),
            disconnected_fired: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl PeerCallbacks {
    /// Create new PeerCallbacks with the specified callbacks
    pub fn new(
        on_connected: Option<OnConnectedCallback>,
        on_disconnected: Option<OnDisconnectedCallback>,
        on_error: Option<OnErrorCallback>,
        user: *mut c_void,
    ) -> Self {
        Self {
            on_connected,
            on_disconnected,
            on_error,
            user,
            disconnected_fired: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl PeerCallbacks {
    /// Fire the connected callback
    pub fn fire_connected(&self) {
        // Reset disconnected flag so it can fire again on next disconnect
        self.reset_disconnected_flag();
        if let Some(cb) = self.on_connected {
            unsafe { cb(self.user); }
        }
    }

    /// Fire the disconnected callback (only fires once per connection)
    pub fn fire_disconnected(&self) {
        // Only fire if not already fired (compare_exchange returns Ok if we swapped false->true)
        if self.disconnected_fired.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
            if let Some(cb) = self.on_disconnected {
                unsafe { cb(self.user); }
            }
        }
    }

    /// Reset the disconnected flag (call when connection is re-established)
    pub fn reset_disconnected_flag(&self) {
        self.disconnected_fired.store(false, Ordering::SeqCst);
    }

    /// Fire the error callback with error code and message
    pub fn fire_error(&self, code: u32, msg: &str) {
        if let Some(cb) = self.on_error {
            if let Ok(c_msg) = CString::new(msg) {
                unsafe { cb(code, c_msg.as_ptr(), self.user); }
            }
        }
    }
}

/// WebRTC peer that connects via WebSocket signaling
pub struct WebRtcPeer {
    /// WebSocket signaling base URL (without room ID)
    signaling_url: String,
    /// Room ID for signaling (appended to URL path)
    room_id: String,
    /// WebRTC peer connection
    peer_connection: Option<Arc<RTCPeerConnection>>,
    /// Outgoing audio track
    audio_track: Option<Arc<TrackLocalStaticSample>>,
    /// Ring buffer consumer for incoming audio
    incoming_consumer: Option<ringbuf::HeapCons<f32>>,
    /// DataChannel
    data_channel: Option<Arc<RTCDataChannel>>,
    /// Connection state
    connected: Arc<AtomicBool>,
    /// WebSocket sender for signaling
    ws_sender: Option<mpsc::UnboundedSender<Message>>,
    /// Statistics
    pub stats: Arc<WebRtcPeerStats>,
    /// DataChannel callback
    data_callback: Option<DataChannelCallback>,
    /// FFI callbacks for peer events
    callbacks: Arc<ParkingMutex<PeerCallbacks>>,
    /// ICE servers
    ice_servers: Vec<IceServerConfig>,
    /// Audio sample rate
    sample_rate: u32,
    /// Audio channels
    channels: u16,
    /// Buffer size in samples
    buffer_samples: usize,
}

impl WebRtcPeer {
    /// Create a new WebRTC peer with room-based signaling
    ///
    /// # Arguments
    /// * `signaling_url` - Base WebSocket URL (e.g., "ws://127.0.0.1:8080")
    /// * `room_id` - Room identifier for signaling isolation (e.g., "studio-1")
    /// * `ice_servers` - ICE server configurations for STUN/TURN
    /// * `sample_rate` - Audio sample rate (typically 48000)
    /// * `channels` - Number of audio channels (1 or 2)
    /// * `buffer_samples` - Size of the incoming audio buffer in samples
    ///
    /// The peer will connect to: `{signaling_url}/{room_id}`
    /// Messages are only exchanged with other clients in the same room.
    pub fn new(
        signaling_url: &str,
        room_id: &str,
        ice_servers: Vec<IceServerConfig>,
        sample_rate: u32,
        channels: u16,
        buffer_samples: usize,
    ) -> Self {
        Self {
            signaling_url: signaling_url.to_string(),
            room_id: room_id.to_string(),
            peer_connection: None,
            audio_track: None,
            incoming_consumer: None,
            data_channel: None,
            connected: Arc::new(AtomicBool::new(false)),
            ws_sender: None,
            stats: Arc::new(WebRtcPeerStats::default()),
            data_callback: None,
            callbacks: Arc::new(ParkingMutex::new(PeerCallbacks::default())),
            ice_servers,
            sample_rate,
            channels,
            buffer_samples,
        }
    }

    /// Set FFI callbacks for peer events
    pub fn set_callbacks(&self, callbacks: PeerCallbacks) {
        *self.callbacks.lock() = callbacks;
    }

    /// Get a clone of the callbacks Arc for use in handlers
    pub fn callbacks(&self) -> Arc<ParkingMutex<PeerCallbacks>> {
        self.callbacks.clone()
    }

    /// Get the room ID this peer is connecting to
    pub fn room_id(&self) -> &str {
        &self.room_id
    }

    /// Set the DataChannel callback
    pub fn set_data_callback(&mut self, callback: DataChannelCallback) {
        self.data_callback = Some(callback);
    }

    /// Check if connected
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    /// Get the peer connection (for cleanup)
    pub fn peer_connection(&self) -> Option<&Arc<RTCPeerConnection>> {
        self.peer_connection.as_ref()
    }

    /// Get the audio track for writing samples
    pub fn audio_track(&self) -> Option<&Arc<TrackLocalStaticSample>> {
        self.audio_track.as_ref()
    }

    /// Take the incoming audio consumer
    pub fn take_incoming_consumer(&mut self) -> Option<ringbuf::HeapCons<f32>> {
        self.incoming_consumer.take()
    }

    /// Send data on a DataChannel
    pub async fn send_data(&self, channel: &str, data: &[u8]) -> Result<(), String> {
        if let Some(ref dc) = self.data_channel {
            // TODO: support multiple channels by name
            let _ = channel;
            dc.send(&bytes::Bytes::copy_from_slice(data))
                .await
                .map_err(|e| format!("Failed to send data: {}", e))?;
            Ok(())
        } else {
            Err("DataChannel not available".to_string())
        }
    }

    /// Connect to the signaling server and wait for WebRTC connection
    pub async fn connect(&mut self) -> Result<(), String> {
        // Build full signaling URL with room ID
        let full_url = format!("{}/{}", self.signaling_url.trim_end_matches('/'), self.room_id);

        // Connect to WebSocket signaling server
        let (ws_stream, _) = tokio_tungstenite::connect_async(&full_url)
            .await
            .map_err(|e| format!("Failed to connect to signaling server: {}", e))?;

        println!("[WebRtcPeer] Connected to signaling server: {} (room: '{}')",
                 full_url, self.room_id);

        let (mut ws_sender, mut ws_receiver) = ws_stream.split();

        // Create channel for sending messages
        let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
        self.ws_sender = Some(tx.clone());

        // Spawn WebSocket sender task
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if ws_sender.send(msg).await.is_err() {
                    break;
                }
            }
        });

        // Create peer connection
        let peer_connection = self.create_peer_connection().await?;
        self.peer_connection = Some(peer_connection.clone());

        // Create outgoing audio track
        let audio_track = Arc::new(TrackLocalStaticSample::new(
            RTCRtpCodecCapability {
                mime_type: "audio/opus".to_owned(),
                clock_rate: self.sample_rate,
                channels: self.channels,
                sdp_fmtp_line: "minptime=10;useinbandfec=1".to_owned(),
                rtcp_feedback: vec![],
            },
            "audio".to_owned(),
            "bass-webrtc".to_owned(),
        ));
        self.audio_track = Some(audio_track.clone());

        // Add sendrecv transceiver
        let _transceiver = peer_connection
            .add_transceiver_from_track(
                audio_track.clone() as Arc<dyn TrackLocal + Send + Sync>,
                Some(webrtc::rtp_transceiver::RTCRtpTransceiverInit {
                    direction: RTCRtpTransceiverDirection::Sendrecv,
                    send_encodings: vec![],
                }),
            )
            .await
            .map_err(|e| format!("Failed to add transceiver: {}", e))?;

        // Create ring buffer for incoming audio
        let rb = HeapRb::<f32>::new(self.buffer_samples);
        let (producer, consumer) = rb.split();
        self.incoming_consumer = Some(consumer);

        // Setup on_track handler for incoming audio
        let producer = Arc::new(parking_lot::Mutex::new(Some(producer)));
        let stats = self.stats.clone();
        let sample_rate = self.sample_rate;
        let channels = self.channels;

        peer_connection.on_track(Box::new(
            move |track: Arc<TrackRemote>, _receiver: Arc<RTCRtpReceiver>, _transceiver: Arc<RTCRtpTransceiver>| {
                let codec = track.codec();

                if !codec.capability.mime_type.to_lowercase().contains("opus") {
                    return Box::pin(async {});
                }

                let producer = match producer.lock().take() {
                    Some(p) => p,
                    None => return Box::pin(async {}),
                };

                let stats = stats.clone();

                Box::pin(async move {
                    spawn_track_reader(track, producer, stats, sample_rate, channels).await;
                })
            },
        ));

        // Setup connection state handler
        let connected = self.connected.clone();
        let callbacks_for_state = self.callbacks.clone();
        peer_connection.on_peer_connection_state_change(Box::new(move |state: RTCPeerConnectionState| {
            println!("[WebRtcPeer] Connection state: {:?}", state);
            let cbs = callbacks_for_state.lock().clone();
            match state {
                RTCPeerConnectionState::Connected => {
                    connected.store(true, Ordering::SeqCst);
                    cbs.fire_connected();
                }
                RTCPeerConnectionState::Disconnected
                | RTCPeerConnectionState::Closed => {
                    connected.store(false, Ordering::SeqCst);
                    cbs.fire_disconnected();
                }
                RTCPeerConnectionState::Failed => {
                    connected.store(false, Ordering::SeqCst);
                    cbs.fire_error(1, "WebRTC connection failed");
                    cbs.fire_disconnected();
                }
                _ => {}
            }
            Box::pin(async {})
        }));

        // Setup ICE candidate handler
        let tx_for_ice = tx.clone();
        peer_connection.on_ice_candidate(Box::new(move |candidate| {
            let tx = tx_for_ice.clone();
            Box::pin(async move {
                if let Some(c) = candidate {
                    let msg = SignalingMessage::Ice {
                        candidate: c.to_json().map(|j| j.candidate).unwrap_or_default(),
                        sdp_m_line_index: c.to_json().ok().and_then(|j| j.sdp_mline_index),
                        sdp_mid: c.to_json().ok().and_then(|j| j.sdp_mid),
                    };
                    if let Ok(json) = serde_json::to_string(&msg) {
                        let _ = tx.send(Message::Text(json));
                    }
                }
            })
        }));

        // Setup DataChannel handler (for incoming channels from browser)
        let data_callback = self.data_callback.clone();
        peer_connection.on_data_channel(Box::new(move |dc: Arc<RTCDataChannel>| {
            let callback = data_callback.clone();
            Box::pin(async move {
                if let Some(cb) = callback {
                    let channel_name = dc.label().to_string();
                    dc.on_message(Box::new(move |msg: DataChannelMessage| {
                        cb(&channel_name, &msg.data);
                        Box::pin(async {})
                    }));
                }
            })
        }));

        // Queue for ICE candidates that arrive before we have a remote description
        let mut pending_ice_candidates: Vec<RTCIceCandidateInit> = Vec::new();
        let mut remote_description_set = false;

        // Process signaling messages
        let pc = peer_connection.clone();
        let tx_for_answer = tx;
        let connected_for_loop = self.connected.clone();

        // Wait for connection to be established
        loop {
            // Use select with timeout to periodically check connection state
            tokio::select! {
                msg_result = ws_receiver.next() => {
                    let msg_result = match msg_result {
                        Some(r) => r,
                        None => {
                            println!("[WebRtcPeer] WebSocket stream ended");
                            break;
                        }
                    };

                    match msg_result {
                        Ok(Message::Text(text)) => {
                            match serde_json::from_str::<SignalingMessage>(&text) {
                                Ok(SignalingMessage::Offer { sdp }) => {
                                    println!("[WebRtcPeer] Received offer");

                                    // Set remote description (offer)
                                    let offer = RTCSessionDescription::offer(sdp)
                                        .map_err(|e| format!("Invalid offer SDP: {}", e))?;
                                    pc.set_remote_description(offer)
                                        .await
                                        .map_err(|e| format!("Failed to set remote description: {}", e))?;

                                    remote_description_set = true;

                                    // Apply any pending ICE candidates
                                    for ice_candidate in pending_ice_candidates.drain(..) {
                                        if let Err(e) = pc.add_ice_candidate(ice_candidate).await {
                                            eprintln!("[WebRtcPeer] Failed to add queued ICE candidate: {}", e);
                                        }
                                    }

                                    // Create answer
                                    let answer = pc.create_answer(None)
                                        .await
                                        .map_err(|e| format!("Failed to create answer: {}", e))?;

                                    // Set local description
                                    pc.set_local_description(answer.clone())
                                        .await
                                        .map_err(|e| format!("Failed to set local description: {}", e))?;

                                    // Wait briefly for ICE gathering
                                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

                                    // Get local description with ICE candidates
                                    if let Some(local_desc) = pc.local_description().await {
                                        let msg = SignalingMessage::Answer { sdp: local_desc.sdp };
                                        if let Ok(json) = serde_json::to_string(&msg) {
                                            let _ = tx_for_answer.send(Message::Text(json));
                                            println!("[WebRtcPeer] Sent answer");
                                        }
                                    }
                                }
                                Ok(SignalingMessage::Ice { candidate, sdp_m_line_index, sdp_mid }) => {
                                    if !candidate.is_empty() {
                                        let ice_candidate = RTCIceCandidateInit {
                                            candidate,
                                            sdp_mid,
                                            sdp_mline_index: sdp_m_line_index,
                                            ..Default::default()
                                        };

                                        if remote_description_set {
                                            // Apply immediately
                                            if let Err(e) = pc.add_ice_candidate(ice_candidate).await {
                                                eprintln!("[WebRtcPeer] Failed to add ICE candidate: {}", e);
                                            }
                                        } else {
                                            // Queue for later
                                            pending_ice_candidates.push(ice_candidate);
                                        }
                                    }
                                }
                                Ok(SignalingMessage::Answer { .. }) => {
                                    // We don't expect answers (we're the answerer)
                                }
                                Ok(SignalingMessage::Data { .. }) => {
                                    // Pre-connection data - ignore for now
                                }
                                Err(e) => {
                                    eprintln!("[WebRtcPeer] Failed to parse signaling message: {}", e);
                                }
                            }
                        }
                        Ok(Message::Close(_)) => {
                            println!("[WebRtcPeer] Signaling connection closed");
                            break;
                        }
                        Ok(_) => {}
                        Err(e) => {
                            eprintln!("[WebRtcPeer] WebSocket error: {}", e);
                            break;
                        }
                    }
                }
                // Check connection state every 100ms even if no messages arrive
                _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                    // Exit once connected - the WebRTC connection is now independent of signaling
                    if connected_for_loop.load(Ordering::SeqCst) {
                        println!("[WebRtcPeer] WebRTC connected, signaling complete");
                        // Send a proper close frame before exiting
                        if let Some(ref sender) = self.ws_sender {
                            let _ = sender.send(Message::Close(None));
                        }
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    /// Create the RTCPeerConnection with default configuration
    async fn create_peer_connection(&self) -> Result<Arc<RTCPeerConnection>, String> {
        let mut media_engine = MediaEngine::default();
        media_engine
            .register_default_codecs()
            .map_err(|e| format!("Failed to register codecs: {}", e))?;

        let mut registry = Registry::new();
        registry = register_default_interceptors(registry, &mut media_engine)
            .map_err(|e| format!("Failed to register interceptors: {}", e))?;

        let api = APIBuilder::new()
            .with_media_engine(media_engine)
            .with_interceptor_registry(registry)
            .build();

        let rtc_config = RTCConfiguration {
            ice_servers: self.ice_servers
                .iter()
                .map(|s| RTCIceServer {
                    urls: s.urls.clone(),
                    username: s.username.clone().unwrap_or_default(),
                    credential: s.credential.clone().unwrap_or_default(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };

        let peer_connection = api
            .new_peer_connection(rtc_config)
            .await
            .map_err(|e| format!("Failed to create peer connection: {}", e))?;

        Ok(Arc::new(peer_connection))
    }

    /// Disconnect from the WebRTC peer
    pub async fn disconnect(&mut self) -> Result<(), String> {
        if let Some(ref pc) = self.peer_connection {
            pc.close()
                .await
                .map_err(|e| format!("Failed to close peer connection: {}", e))?;
        }
        self.peer_connection = None;
        self.ws_sender = None;
        self.connected.store(false, Ordering::SeqCst);
        Ok(())
    }
}

/// Spawn an async task to read RTP from a remote track and decode OPUS to PCM.
async fn spawn_track_reader(
    track: Arc<TrackRemote>,
    mut producer: ringbuf::HeapProd<f32>,
    stats: Arc<WebRtcPeerStats>,
    sample_rate: u32,
    channels: u16,
) {
    let format = AudioFormat::new(sample_rate, channels as u8);
    let mut decoder = match OpusDecoder::new(format, 20.0) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("[WebRtcPeer] Failed to create OPUS decoder: {:?}", e);
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
                        let _pushed = producer.push_slice(&pcm_buffer[..total_samples]);

                        stats.packets_received.fetch_add(1, Ordering::Relaxed);
                        stats.bytes_received.fetch_add(payload.len() as u64, Ordering::Relaxed);
                    }
                    Err(_e) => {
                        // Decode error - skip this packet
                    }
                }
            }
            Err(e) => {
                let err_str = e.to_string().to_lowercase();
                if err_str.contains("eof")
                    || err_str.contains("closed")
                    || err_str.contains("nil")
                    || err_str.contains("must not be")
                {
                    break;
                }
                eprintln!("[WebRtcPeer] RTP read error: {}", e);
            }
        }
    }
}

impl Drop for WebRtcPeer {
    fn drop(&mut self) {
        // Note: Can't do async cleanup in drop
        // Caller should call disconnect() before dropping
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signaling_message_serialize() {
        let offer = SignalingMessage::Offer { sdp: "v=0\r\n".to_string() };
        let json = serde_json::to_string(&offer).unwrap();
        assert!(json.contains("\"type\":\"offer\""));
        assert!(json.contains("\"sdp\":\"v=0\\r\\n\""));
    }

    #[test]
    fn test_signaling_message_deserialize() {
        let json = r#"{"type":"ice","candidate":"candidate:1 1 UDP 2130706431 192.168.1.1 8189 typ host","sdpMLineIndex":0}"#;
        let msg: SignalingMessage = serde_json::from_str(json).unwrap();
        match msg {
            SignalingMessage::Ice { candidate, sdp_m_line_index, .. } => {
                assert!(candidate.contains("candidate:"));
                assert_eq!(sdp_m_line_index, Some(0));
            }
            _ => panic!("Expected Ice message"),
        }
    }
}
