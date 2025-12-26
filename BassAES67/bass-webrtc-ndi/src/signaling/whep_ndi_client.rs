//! WHEP NDI Client - WebRTC-HTTP Egress Protocol client with NDI video output.
//!
//! Connects to an external WHEP server (like MediaMTX) to pull audio AND video.
//! - Audio: Decoded via OPUS -> ring buffer -> BASS channel (user handles playback)
//! - Video: Decoded via FFmpeg -> VideoFrame -> NdiSender
//!
//! Flow:
//! 1. Create RTCPeerConnection with recv-only configuration for audio and video
//! 2. Create SDP offer with recvonly transceivers
//! 3. POST offer to WHEP endpoint
//! 4. Set remote answer from response
//! 5. Receive media via on_track handler:
//!    - Audio tracks -> OPUS decoder -> ring buffer
//!    - Video tracks -> FFmpeg decoder -> NDI sender

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{Method, Request, StatusCode};
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use parking_lot::Mutex;
use ringbuf::traits::{Producer, Split};
use ringbuf::HeapRb;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::APIBuilder;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::rtp_transceiver::rtp_receiver::RTCRtpReceiver;
use webrtc::rtp_transceiver::rtp_transceiver_direction::RTCRtpTransceiverDirection;
use webrtc::rtp_transceiver::RTCRtpTransceiver;
use webrtc::track::track_remote::TrackRemote;

use crate::codec::opus::Decoder as OpusDecoder;
use crate::codec::video::{VideoCodec, VideoDecoder};
use crate::codec::AudioFormat;
use crate::frame::{AudioFrame, VideoFormat};
use crate::peer::IceServerConfig;
use crate::sender::{NdiSender, init_ndi};

use grafton_ndi::NDI;

/// Configuration for WHEP NDI client
#[derive(Clone)]
pub struct WhepNdiConfig {
    /// WHEP endpoint URL
    pub endpoint_url: String,
    /// ICE servers for NAT traversal
    pub ice_servers: Vec<IceServerConfig>,
    /// Audio sample rate (48000)
    pub sample_rate: u32,
    /// Number of audio channels (1 or 2)
    pub channels: u16,
    /// Size of incoming audio buffer in samples
    pub buffer_samples: usize,
    /// NDI source name (None = no NDI output)
    pub ndi_name: Option<String>,
    /// Send audio to NDI (in addition to BASS ring buffer)
    pub audio_to_ndi: bool,
}

/// Wrapper to hold NDI instance and sender together
struct NdiContext {
    _ndi: Arc<NDI>,
    sender: NdiSender<'static>,
}

impl NdiContext {
    fn new(name: &str) -> Result<Self, String> {
        let ndi = init_ndi().map_err(|e| format!("Failed to init NDI: {}", e))?;

        // Safety: We store the Arc<NDI> alongside the sender, so it lives long enough.
        // We use 'static lifetime and leak the Arc to make this work.
        let ndi_static: &'static Arc<NDI> = Box::leak(Box::new(ndi.clone()));

        let sender = NdiSender::new(ndi_static, name)
            .map_err(|e| format!("Failed to create sender: {}", e))?;

        Ok(Self {
            _ndi: ndi,
            sender,
        })
    }
}

/// WHEP NDI client statistics
pub struct WhepNdiClientStats {
    // Audio stats
    pub audio_packets_received: AtomicU64,
    pub audio_bytes_received: AtomicU64,
    pub audio_decode_errors: AtomicU64,
    pub audio_frames_sent_ndi: AtomicU64,
    // Video stats
    pub video_packets_received: AtomicU64,
    pub video_bytes_received: AtomicU64,
    pub video_frames_decoded: AtomicU64,
    pub video_frames_sent_ndi: AtomicU64,
    pub video_decode_errors: AtomicU64,
}

impl Default for WhepNdiClientStats {
    fn default() -> Self {
        Self {
            audio_packets_received: AtomicU64::new(0),
            audio_bytes_received: AtomicU64::new(0),
            audio_decode_errors: AtomicU64::new(0),
            audio_frames_sent_ndi: AtomicU64::new(0),
            video_packets_received: AtomicU64::new(0),
            video_bytes_received: AtomicU64::new(0),
            video_frames_decoded: AtomicU64::new(0),
            video_frames_sent_ndi: AtomicU64::new(0),
            video_decode_errors: AtomicU64::new(0),
        }
    }
}

/// WHEP NDI client for pulling audio+video from an external WHEP server
pub struct WhepNdiClient {
    /// WHEP endpoint URL
    endpoint_url: String,
    /// WebRTC peer connection
    peer_connection: Arc<RTCPeerConnection>,
    /// Ring buffer consumer for incoming audio
    incoming_consumer: Option<ringbuf::HeapCons<f32>>,
    /// Resource URL returned by server (for DELETE on close)
    resource_url: Option<String>,
    /// Connection state
    connected: Arc<AtomicBool>,
    /// Statistics
    pub stats: Arc<WhepNdiClientStats>,
    /// NDI context (holds NDI instance and sender)
    ndi_context: Option<Arc<NdiContext>>,
    /// Whether audio is sent to NDI
    audio_to_ndi: bool,
}

impl WhepNdiClient {
    /// Create and connect a new WHEP NDI client using config struct.
    ///
    /// # Arguments
    /// * `config` - Configuration for the WHEP NDI client
    pub async fn connect_with_config(config: &WhepNdiConfig) -> Result<Self, String> {
        Self::connect(
            &config.endpoint_url,
            &config.ice_servers,
            config.sample_rate,
            config.channels,
            config.buffer_samples,
            config.ndi_name.as_deref(),
            config.audio_to_ndi,
        )
        .await
    }

    /// Create and connect a new WHEP NDI client.
    ///
    /// # Arguments
    /// * `endpoint_url` - WHEP endpoint URL (e.g., "http://localhost:8889/mystream/whep")
    /// * `ice_servers` - List of ICE servers for NAT traversal
    /// * `sample_rate` - Audio sample rate (48000)
    /// * `channels` - Number of channels (1 or 2)
    /// * `buffer_samples` - Size of incoming audio buffer in samples
    /// * `ndi_name` - Optional NDI source name (enables NDI output if provided)
    /// * `audio_to_ndi` - If true, audio is also sent to NDI (in addition to BASS ring buffer)
    pub async fn connect(
        endpoint_url: &str,
        ice_servers: &[IceServerConfig],
        sample_rate: u32,
        channels: u16,
        buffer_samples: usize,
        ndi_name: Option<&str>,
        audio_to_ndi: bool,
    ) -> Result<Self, String> {
        // Create NDI context (NDI instance + sender) if name provided
        let ndi_context = if let Some(name) = ndi_name {
            match NdiContext::new(name) {
                Ok(ctx) => {
                    println!("WhepNdiClient: Created NDI sender '{}' (audio_to_ndi={})", name, audio_to_ndi);
                    Some(Arc::new(ctx))
                }
                Err(e) => {
                    eprintln!("WhepNdiClient: Failed to create NDI context: {}", e);
                    None
                }
            }
        } else {
            None
        };

        // Create media engine and register default codecs (audio + video)
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

        // Build RTCConfiguration with ICE servers
        let rtc_config = RTCConfiguration {
            ice_servers: ice_servers
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

        // Create peer connection
        let peer_connection = api
            .new_peer_connection(rtc_config)
            .await
            .map_err(|e| format!("Failed to create peer connection: {}", e))?;

        let peer_connection = Arc::new(peer_connection);

        // Add recv-only transceiver for AUDIO
        peer_connection
            .add_transceiver_from_kind(
                webrtc::rtp_transceiver::rtp_codec::RTPCodecType::Audio,
                Some(webrtc::rtp_transceiver::RTCRtpTransceiverInit {
                    direction: RTCRtpTransceiverDirection::Recvonly,
                    send_encodings: vec![],
                }),
            )
            .await
            .map_err(|e| format!("Failed to add audio transceiver: {}", e))?;

        // Add recv-only transceiver for VIDEO (only if NDI context is available)
        if ndi_context.is_some() {
            peer_connection
                .add_transceiver_from_kind(
                    webrtc::rtp_transceiver::rtp_codec::RTPCodecType::Video,
                    Some(webrtc::rtp_transceiver::RTCRtpTransceiverInit {
                        direction: RTCRtpTransceiverDirection::Recvonly,
                        send_encodings: vec![],
                    }),
                )
                .await
                .map_err(|e| format!("Failed to add video transceiver: {}", e))?;
        }

        // Create ring buffer for incoming audio
        let rb = HeapRb::<f32>::new(buffer_samples);
        let (producer, consumer) = rb.split();

        // Statistics
        let stats = Arc::new(WhepNdiClientStats::default());

        // Track arrival notification
        // Bit 0 = audio track arrived, Bit 1 = video track arrived
        let tracks_arrived = Arc::new(AtomicU8::new(0));
        let tracks_notify = Arc::new(tokio::sync::Notify::new());

        // Determine which tracks we expect
        let expect_video = ndi_context.is_some();

        // Setup on_track handler for incoming media
        let audio_producer = Arc::new(Mutex::new(Some(producer)));
        let stats_for_track = stats.clone();
        let ndi_for_track = ndi_context.clone();
        let ndi_for_audio = if audio_to_ndi { ndi_context.clone() } else { None };
        let track_sample_rate = sample_rate;
        let track_channels = channels;
        let tracks_arrived_clone = tracks_arrived.clone();
        let tracks_notify_clone = tracks_notify.clone();

        peer_connection.on_track(Box::new(
            move |track: Arc<TrackRemote>, _receiver: Arc<RTCRtpReceiver>, _transceiver: Arc<RTCRtpTransceiver>| {
                let codec = track.codec();
                let mime_type = codec.capability.mime_type.to_lowercase();

                // Handle AUDIO tracks (OPUS)
                if mime_type.contains("opus") {
                    let producer = match audio_producer.lock().take() {
                        Some(p) => p,
                        None => {
                            eprintln!("WhepNdiClient: on_track audio called but producer already taken");
                            return Box::pin(async {});
                        }
                    };

                    // Signal that audio track arrived (bit 0)
                    tracks_arrived_clone.fetch_or(0x01, Ordering::SeqCst);
                    tracks_notify_clone.notify_one();

                    let stats = stats_for_track.clone();
                    let ndi = ndi_for_audio.clone();
                    let sample_rate = track_sample_rate;
                    let channels = track_channels;

                    return Box::pin(async move {
                        spawn_audio_track_reader(track, producer, ndi, stats, sample_rate, channels).await;
                    });
                }

                // Handle VIDEO tracks (H.264, VP8, VP9)
                if mime_type.contains("h264") || mime_type.contains("vp8") || mime_type.contains("vp9") {
                    let ndi_ctx = match ndi_for_track.clone() {
                        Some(ctx) => ctx,
                        None => {
                            eprintln!("WhepNdiClient: on_track video called but no NDI context");
                            return Box::pin(async {});
                        }
                    };

                    // Signal that video track arrived (bit 1)
                    tracks_arrived_clone.fetch_or(0x02, Ordering::SeqCst);
                    tracks_notify_clone.notify_one();

                    let video_codec = if mime_type.contains("h264") {
                        VideoCodec::H264
                    } else if mime_type.contains("vp8") {
                        VideoCodec::VP8
                    } else {
                        VideoCodec::VP9
                    };

                    let stats = stats_for_track.clone();

                    return Box::pin(async move {
                        spawn_video_track_reader(track, ndi_ctx, video_codec, stats).await;
                    });
                }

                // Unknown track type
                eprintln!("WhepNdiClient: Ignoring unknown track type: {}", mime_type);
                Box::pin(async {})
            },
        ));

        // Setup connection state handler
        let connected = Arc::new(AtomicBool::new(false));
        let connected_clone = connected.clone();
        peer_connection.on_peer_connection_state_change(Box::new(move |state: RTCPeerConnectionState| {
            match state {
                RTCPeerConnectionState::Connected => {
                    connected_clone.store(true, Ordering::SeqCst);
                }
                RTCPeerConnectionState::Disconnected
                | RTCPeerConnectionState::Failed
                | RTCPeerConnectionState::Closed => {
                    connected_clone.store(false, Ordering::SeqCst);
                }
                _ => {}
            }
            Box::pin(async {})
        }));

        // Wait for ICE gathering to complete before creating offer
        let ice_complete = Arc::new(tokio::sync::Notify::new());
        let ice_complete_clone = ice_complete.clone();
        peer_connection.on_ice_gathering_state_change(Box::new(move |state| {
            if state == webrtc::ice_transport::ice_gatherer_state::RTCIceGathererState::Complete {
                ice_complete_clone.notify_one();
            }
            Box::pin(async {})
        }));

        // Create offer
        let offer = peer_connection
            .create_offer(None)
            .await
            .map_err(|e| format!("Failed to create offer: {}", e))?;

        // Set local description
        peer_connection
            .set_local_description(offer)
            .await
            .map_err(|e| format!("Failed to set local description: {}", e))?;

        // Wait for ICE gathering (with timeout)
        tokio::select! {
            _ = ice_complete.notified() => {}
            _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {}
        }

        // Get the local description with gathered ICE candidates
        let local_desc = peer_connection
            .local_description()
            .await
            .ok_or("No local description available")?;

        // POST offer to WHEP endpoint
        let (answer_sdp, resource_url) = Self::post_offer(endpoint_url, &local_desc.sdp).await?;

        // Set remote description (answer)
        let answer = RTCSessionDescription::answer(answer_sdp.clone())
            .map_err(|e| format!("Invalid answer SDP: {}", e))?;

        peer_connection
            .set_remote_description(answer)
            .await
            .map_err(|e| format!("Failed to set remote description: {}", e))?;

        // Wait for tracks to arrive via on_track callback
        // Expected tracks: audio (0x01), and optionally video (0x02)
        let expected_tracks = if expect_video { 0x03 } else { 0x01 };
        let track_start = std::time::Instant::now();

        // First, wait up to 2 seconds for on_track callbacks
        let initial_wait = Duration::from_secs(2);
        loop {
            let arrived = tracks_arrived.load(Ordering::SeqCst);
            if arrived >= expected_tracks {
                break;
            }

            if track_start.elapsed() > initial_wait {
                // Time to check transceivers directly
                break;
            }

            tokio::select! {
                _ = tracks_notify.notified() => {
                    // Track arrived, loop back to check
                }
                _ = tokio::time::sleep(Duration::from_millis(100)) => {
                    // Keep checking
                }
            }
        }

        // After initial wait, check transceivers for any tracks that didn't trigger on_track
        // This works around a webrtc-rs bug where only the first track fires on_track
        let arrived = tracks_arrived.load(Ordering::SeqCst);
        let has_audio_via_callback = (arrived & 0x01) != 0;
        let has_video_via_callback = (arrived & 0x02) != 0;

        if arrived < expected_tracks {
            let transceivers = peer_connection.get_transceivers().await;

            // Check for video track that didn't fire on_track
            if !has_video_via_callback && expect_video {
                for t in transceivers.iter() {
                    if t.kind() == webrtc::rtp_transceiver::rtp_codec::RTPCodecType::Video {
                        let receiver = t.receiver().await;
                        let tracks = receiver.tracks().await;
                        for track in tracks.iter() {
                            let codec = track.codec();
                            let mime_type = codec.capability.mime_type.to_lowercase();

                            if let Some(ref ndi_ctx) = ndi_context {
                                let video_codec = if mime_type.contains("h264") {
                                    VideoCodec::H264
                                } else if mime_type.contains("vp8") {
                                    VideoCodec::VP8
                                } else {
                                    VideoCodec::VP9
                                };
                                let stats_clone = stats.clone();
                                let ndi_ctx_clone = ndi_ctx.clone();
                                let track_clone = track.clone();
                                tokio::spawn(async move {
                                    spawn_video_track_reader(track_clone, ndi_ctx_clone, video_codec, stats_clone).await;
                                });
                                // Mark video as arrived since we're handling it
                                tracks_arrived.fetch_or(0x02, Ordering::SeqCst);
                            }
                        }
                    }
                }
            }

            // Check for audio track that didn't fire on_track (less common but possible)
            if !has_audio_via_callback {
                for t in transceivers.iter() {
                    if t.kind() == webrtc::rtp_transceiver::rtp_codec::RTPCodecType::Audio {
                        let receiver = t.receiver().await;
                        let tracks = receiver.tracks().await;
                        if !tracks.is_empty() {
                            // Audio needs the producer which was already taken or not available here
                            // Just mark it as present - the on_track handler should have it
                            tracks_arrived.fetch_or(0x01, Ordering::SeqCst);
                        }
                    }
                }
            }
        }

        // Final check
        let final_arrived = tracks_arrived.load(Ordering::SeqCst);
        let has_audio = (final_arrived & 0x01) != 0;
        let has_video = (final_arrived & 0x02) != 0;

        if !has_audio && !has_video {
            return Err("No media tracks available".to_string());
        }

        Ok(Self {
            endpoint_url: endpoint_url.to_string(),
            peer_connection,
            incoming_consumer: Some(consumer),
            resource_url,
            connected,
            stats,
            ndi_context,
            audio_to_ndi,
        })
    }

    /// Check if audio is being sent to NDI
    pub fn is_audio_to_ndi(&self) -> bool {
        self.audio_to_ndi
    }

    /// Check if NDI is available
    pub fn has_ndi(&self) -> bool {
        self.ndi_context.is_some()
    }

    /// POST SDP offer to WHEP endpoint
    async fn post_offer(endpoint_url: &str, offer_sdp: &str) -> Result<(String, Option<String>), String> {
        // Create HTTP client with webpki roots for TLS
        let https = hyper_rustls::HttpsConnectorBuilder::new()
            .with_webpki_roots()
            .https_or_http()
            .enable_http1()
            .build();

        let client: Client<_, Full<Bytes>> = Client::builder(TokioExecutor::new()).build(https);

        // Build request
        let request = Request::builder()
            .method(Method::POST)
            .uri(endpoint_url)
            .header("Content-Type", "application/sdp")
            .body(Full::new(Bytes::from(offer_sdp.to_string())))
            .map_err(|e| format!("Failed to build request: {}", e))?;

        // Send request
        let response = client
            .request(request)
            .await
            .map_err(|e| format!("WHEP request failed: {}", e))?;

        let status = response.status();

        // Get resource URL from Location header
        let resource_url = response
            .headers()
            .get("Location")
            .and_then(|v| v.to_str().ok())
            .map(|s| {
                if s.starts_with("http") {
                    s.to_string()
                } else {
                    // Relative URL - resolve against endpoint
                    if let Ok(base) = url::Url::parse(endpoint_url) {
                        base.join(s).map(|u| u.to_string()).unwrap_or_else(|_| s.to_string())
                    } else {
                        s.to_string()
                    }
                }
            });

        // Read response body
        let body_bytes = response
            .into_body()
            .collect()
            .await
            .map_err(|e| format!("Failed to read response body: {}", e))?
            .to_bytes();

        let body_str = String::from_utf8_lossy(&body_bytes).to_string();

        // Check status
        if status != StatusCode::CREATED && status != StatusCode::OK {
            return Err(format!("WHEP server returned {}: {}", status, body_str));
        }

        Ok((body_str, resource_url))
    }

    /// Check if connected
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    /// Take the incoming audio consumer (for use by BASS input stream)
    pub fn take_incoming_consumer(&mut self) -> Option<ringbuf::HeapCons<f32>> {
        self.incoming_consumer.take()
    }

    /// Get number of NDI connections
    pub fn get_ndi_connections(&self) -> u32 {
        self.ndi_context
            .as_ref()
            .map(|ctx| ctx.sender.connection_count())
            .unwrap_or(0)
    }

    /// Check if NDI has any receivers connected
    pub fn has_ndi_connections(&self) -> bool {
        self.ndi_context
            .as_ref()
            .map(|ctx| ctx.sender.has_connections())
            .unwrap_or(false)
    }

    /// Disconnect from the WHEP server
    pub async fn disconnect(&self) -> Result<(), String> {
        // Send DELETE to resource URL if available
        if let Some(ref resource_url) = self.resource_url {
            let _ = Self::delete_resource(resource_url).await;
        }

        // Close peer connection
        self.peer_connection
            .close()
            .await
            .map_err(|e| format!("Failed to close peer connection: {}", e))?;

        Ok(())
    }

    /// Send DELETE request to resource URL
    async fn delete_resource(resource_url: &str) -> Result<(), String> {
        let https = hyper_rustls::HttpsConnectorBuilder::new()
            .with_webpki_roots()
            .https_or_http()
            .enable_http1()
            .build();

        let client: Client<_, Full<Bytes>> = Client::builder(TokioExecutor::new()).build(https);

        let request = Request::builder()
            .method(Method::DELETE)
            .uri(resource_url)
            .body(Full::new(Bytes::new()))
            .map_err(|e| format!("Failed to build DELETE request: {}", e))?;

        let _ = client.request(request).await;

        Ok(())
    }
}

/// Spawn async task to read AUDIO RTP and decode OPUS to PCM
async fn spawn_audio_track_reader(
    track: Arc<TrackRemote>,
    mut producer: ringbuf::HeapProd<f32>,
    ndi_context: Option<Arc<NdiContext>>,
    stats: Arc<WhepNdiClientStats>,
    sample_rate: u32,
    channels: u16,
) {

    // Create OPUS decoder for 20ms frames
    let format = AudioFormat::new(sample_rate, channels as u8);
    let mut decoder = match OpusDecoder::new(format, 20.0) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("WhepNdiClient: Failed to create OPUS decoder: {:?}", e);
            return;
        }
    };

    // Buffer for decoded PCM
    let max_samples = decoder.total_samples_per_frame();
    let mut pcm_buffer = vec![0.0f32; max_samples];

    // Read loop
    loop {
        match track.read_rtp().await {
            Ok((rtp_packet, _attributes)) => {
                let payload = rtp_packet.payload.as_ref();
                if payload.is_empty() {
                    continue;
                }

                // Decode OPUS to f32 PCM
                match decoder.decode_float(payload, &mut pcm_buffer, false) {
                    Ok(samples_per_channel) => {
                        let total_samples = samples_per_channel * channels as usize;

                        // Push decoded samples to ring buffer for BASS (lock-free)
                        let pushed = producer.push_slice(&pcm_buffer[..total_samples]);

                        if pushed < total_samples {
                            stats.audio_decode_errors.fetch_add(1, Ordering::Relaxed);
                        }

                        // Also send to NDI if configured
                        if let Some(ref ndi_ctx) = ndi_context {
                            let audio_frame = AudioFrame {
                                sample_rate,
                                channels,
                                samples_per_channel: samples_per_channel as u32,
                                data: pcm_buffer[..total_samples].to_vec(),
                                timestamp: 0,
                            };
                            if ndi_ctx.sender.send_audio(&audio_frame).is_ok() {
                                stats.audio_frames_sent_ndi.fetch_add(1, Ordering::Relaxed);
                            }
                        }

                        stats.audio_packets_received.fetch_add(1, Ordering::Relaxed);
                        stats.audio_bytes_received.fetch_add(payload.len() as u64, Ordering::Relaxed);
                    }
                    Err(e) => {
                        stats.audio_decode_errors.fetch_add(1, Ordering::Relaxed);
                        eprintln!("WhepNdiClient: OPUS decode error: {:?}", e);
                    }
                }
            }
            Err(e) => {
                let err_str = e.to_string().to_lowercase();
                // Gracefully exit on expected shutdown errors
                if err_str.contains("eof")
                    || err_str.contains("closed")
                    || err_str.contains("nil")
                    || err_str.contains("must not be")
                {
                    break;
                }
                eprintln!("WhepNdiClient: Audio RTP read error: {}", e);
            }
        }
    }
}

/// H.264 RTP Depacketizer state
struct H264Depacketizer {
    /// Buffer for accumulating NAL units in Annex B format
    frame_buffer: Vec<u8>,
    /// Buffer for FU-A fragment reassembly
    fu_buffer: Vec<u8>,
    /// Whether we're currently receiving FU-A fragments
    in_fu: bool,
    /// Last timestamp to detect frame boundaries
    last_timestamp: u32,
}

impl H264Depacketizer {
    fn new() -> Self {
        Self {
            frame_buffer: Vec::with_capacity(512 * 1024), // 512KB for frame
            fu_buffer: Vec::with_capacity(128 * 1024),    // 128KB for single NAL
            in_fu: false,
            last_timestamp: 0,
        }
    }

    /// Process an RTP payload and return complete Annex B frame if ready.
    /// Returns Some(data) when a complete frame is available (marker bit set).
    fn process(&mut self, payload: &[u8], timestamp: u32, marker: bool) -> Option<Vec<u8>> {
        if payload.is_empty() {
            return None;
        }

        // Check if timestamp changed (new frame started)
        if timestamp != self.last_timestamp && !self.frame_buffer.is_empty() {
            // Previous frame is complete, get it
            let frame = std::mem::take(&mut self.frame_buffer);
            self.last_timestamp = timestamp;
            self.in_fu = false;
            self.fu_buffer.clear();

            // Process current payload for new frame
            self.process_payload(payload);

            // Return the previous frame and continue
            if !frame.is_empty() {
                return Some(frame);
            }
        }

        self.last_timestamp = timestamp;
        self.process_payload(payload);

        // If marker bit is set, frame is complete
        if marker && !self.frame_buffer.is_empty() {
            return Some(std::mem::take(&mut self.frame_buffer));
        }

        None
    }

    /// Process a single RTP payload according to RFC 6184
    fn process_payload(&mut self, payload: &[u8]) {
        if payload.is_empty() {
            return;
        }

        let first_byte = payload[0];
        let nal_type = first_byte & 0x1F;

        match nal_type {
            // Single NAL unit (types 1-23)
            1..=23 => {
                // Add start code and NAL unit
                self.frame_buffer.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
                self.frame_buffer.extend_from_slice(payload);
            }
            // STAP-A (type 24) - Single-Time Aggregation Packet
            24 => {
                self.process_stap_a(&payload[1..]);
            }
            // FU-A (type 28) - Fragmentation Unit
            28 => {
                if payload.len() < 2 {
                    return;
                }
                self.process_fu_a(first_byte, payload[1], &payload[2..]);
            }
            // FU-B (type 29) - similar to FU-A but with DON
            29 => {
                if payload.len() < 4 {
                    return;
                }
                // Skip DON (2 bytes) and process like FU-A
                self.process_fu_a(first_byte, payload[1], &payload[4..]);
            }
            // STAP-B (type 25), MTAP16 (type 26), MTAP24 (type 27) - less common
            _ => {
                // Unsupported type, try to pass through
                self.frame_buffer.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
                self.frame_buffer.extend_from_slice(payload);
            }
        }
    }

    /// Process STAP-A packet containing multiple NAL units
    fn process_stap_a(&mut self, data: &[u8]) {
        let mut pos = 0;
        while pos + 2 <= data.len() {
            // Read 16-bit NAL unit size
            let nal_size = ((data[pos] as usize) << 8) | (data[pos + 1] as usize);
            pos += 2;

            if pos + nal_size > data.len() {
                break;
            }

            // Add start code and NAL unit
            self.frame_buffer.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
            self.frame_buffer.extend_from_slice(&data[pos..pos + nal_size]);
            pos += nal_size;
        }
    }

    /// Process FU-A fragment
    fn process_fu_a(&mut self, fu_indicator: u8, fu_header: u8, payload: &[u8]) {
        let start_bit = (fu_header & 0x80) != 0;
        let end_bit = (fu_header & 0x40) != 0;
        let nal_type = fu_header & 0x1F;

        if start_bit {
            // Start of fragmented NAL - reconstruct NAL header
            self.fu_buffer.clear();
            let reconstructed_header = (fu_indicator & 0xE0) | nal_type;
            self.fu_buffer.push(reconstructed_header);
            self.in_fu = true;
        }

        if self.in_fu {
            // Append fragment data
            self.fu_buffer.extend_from_slice(payload);

            if end_bit {
                // End of fragmented NAL - output complete NAL
                self.frame_buffer.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
                self.frame_buffer.extend_from_slice(&self.fu_buffer);
                self.fu_buffer.clear();
                self.in_fu = false;
            }
        }
    }
}

/// Spawn async task to read VIDEO RTP and decode via FFmpeg, then send to NDI
async fn spawn_video_track_reader(
    track: Arc<TrackRemote>,
    ndi_context: Arc<NdiContext>,
    video_codec: VideoCodec,
    stats: Arc<WhepNdiClientStats>,
) {
    // Check if FFmpeg is available
    if !crate::codec::video::is_available() {
        return;
    }

    // Create decoder lazily
    let decoder: Arc<Mutex<Option<VideoDecoder>>> = Arc::new(Mutex::new(None));

    // H.264 RTP depacketizer (converts RTP H.264 to Annex B format)
    let mut h264_depacketizer = H264Depacketizer::new();

    // Read loop
    loop {
        match track.read_rtp().await {
            Ok((rtp_packet, _attributes)) => {
                let payload = rtp_packet.payload.as_ref();
                if payload.is_empty() {
                    continue;
                }

                stats.video_packets_received.fetch_add(1, Ordering::Relaxed);
                stats.video_bytes_received.fetch_add(payload.len() as u64, Ordering::Relaxed);

                let timestamp = rtp_packet.header.timestamp;
                let marker = rtp_packet.header.marker;

                // For H.264, use depacketizer to convert to Annex B format
                // For VP8/VP9, pass through (they have different depacketization needs)
                let frame_data = match video_codec {
                    VideoCodec::H264 => h264_depacketizer.process(payload, timestamp, marker),
                    VideoCodec::VP8 | VideoCodec::VP9 => {
                        // VP8/VP9 - for now pass through, they need their own depacketizers
                        // Most VP8/VP9 RTP payloads can be decoded directly with some care
                        if marker {
                            Some(payload.to_vec())
                        } else {
                            None
                        }
                    }
                };

                // If we have a complete frame, decode it
                if let Some(data) = frame_data {
                    try_decode_and_send(
                        &decoder,
                        &data,
                        video_codec,
                        &ndi_context,
                        &stats,
                    );
                }
            }
            Err(e) => {
                let err_str = e.to_string().to_lowercase();
                // Gracefully exit on expected shutdown errors
                if err_str.contains("eof")
                    || err_str.contains("closed")
                    || err_str.contains("nil")
                    || err_str.contains("must not be")
                {
                    break;
                }
                eprintln!("WhepNdiClient: Video RTP read error: {}", e);
            }
        }
    }
}

/// Try to decode video data and send to NDI
fn try_decode_and_send(
    decoder: &Arc<Mutex<Option<VideoDecoder>>>,
    data: &[u8],
    video_codec: VideoCodec,
    ndi_context: &Arc<NdiContext>,
    stats: &Arc<WhepNdiClientStats>,
) {
    if data.is_empty() {
        return;
    }

    // Get or create decoder
    let mut decoder_guard = decoder.lock();

    // Lazy initialization - create decoder on first frame
    if decoder_guard.is_none() {
        match VideoDecoder::new(video_codec) {
            Ok(dec) => {
                *decoder_guard = Some(dec);
            }
            Err(_) => {
                stats.video_decode_errors.fetch_add(1, Ordering::Relaxed);
                return;
            }
        }
    }

    let dec = decoder_guard.as_mut().unwrap();

    // Decode the frame
    match dec.decode(data) {
        Ok(Some(video_frame)) => {
            stats.video_frames_decoded.fetch_add(1, Ordering::Relaxed);

            // Send to NDI
            if ndi_context.sender.send_video(&video_frame).is_ok() {
                stats.video_frames_sent_ndi.fetch_add(1, Ordering::Relaxed);
            }
        }
        Ok(None) => {
            // No frame produced yet (need more data)
        }
        Err(_) => {
            stats.video_decode_errors.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl Drop for WhepNdiClient {
    fn drop(&mut self) {
        // Note: Can't do async cleanup in drop
        // Caller should call disconnect() before dropping
    }
}
