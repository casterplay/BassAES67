//! WHEP Client - WebRTC-HTTP Egress Protocol client.
//!
//! Connects to an external WHEP server (like MediaMTX) to pull audio.
//! Flow:
//! 1. Create RTCPeerConnection with recv-only configuration
//! 2. Create SDP offer with recvonly
//! 3. POST offer to WHEP endpoint
//! 4. Set remote answer from response
//! 5. Receive audio via on_track handler

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{Method, Request, StatusCode};
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
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
use crate::codec::AudioFormat;
use crate::peer::IceServerConfig;

/// WHEP client statistics
pub struct WhepClientStats {
    pub packets_received: AtomicU64,
    pub bytes_received: AtomicU64,
    pub decode_errors: AtomicU64,
}

impl Default for WhepClientStats {
    fn default() -> Self {
        Self {
            packets_received: AtomicU64::new(0),
            bytes_received: AtomicU64::new(0),
            decode_errors: AtomicU64::new(0),
        }
    }
}

/// WHEP client for pulling audio from an external WHEP server
pub struct WhepClient {
    /// WHEP endpoint URL
    endpoint_url: String,
    /// WebRTC peer connection
    peer_connection: Arc<RTCPeerConnection>,
    /// Ring buffer consumer for incoming audio
    incoming_consumer: Option<ringbuf::HeapCons<f32>>,
    /// Resource URL returned by server (for DELETE on close)
    resource_url: Option<String>,
    /// Connection state
    connected: AtomicBool,
    /// Statistics
    pub stats: Arc<WhepClientStats>,
}

impl WhepClient {
    /// Create and connect a new WHEP client.
    ///
    /// # Arguments
    /// * `endpoint_url` - WHEP endpoint URL (e.g., "http://localhost:8889/mystream/whep")
    /// * `ice_servers` - List of ICE servers for NAT traversal
    /// * `sample_rate` - Audio sample rate (48000)
    /// * `channels` - Number of channels (1 or 2)
    /// * `buffer_samples` - Size of incoming audio buffer in samples
    pub async fn connect(
        endpoint_url: &str,
        ice_servers: &[IceServerConfig],
        sample_rate: u32,
        channels: u16,
        buffer_samples: usize,
    ) -> Result<Self, String> {
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

        // Add recv-only transceiver for audio
        peer_connection
            .add_transceiver_from_kind(
                webrtc::rtp_transceiver::rtp_codec::RTPCodecType::Audio,
                Some(webrtc::rtp_transceiver::RTCRtpTransceiverInit {
                    direction: RTCRtpTransceiverDirection::Recvonly,
                    send_encodings: vec![],
                }),
            )
            .await
            .map_err(|e| format!("Failed to add transceiver: {}", e))?;

        // Create ring buffer for incoming audio
        let rb = HeapRb::<f32>::new(buffer_samples);
        let (producer, consumer) = rb.split();

        // Statistics
        let stats = Arc::new(WhepClientStats::default());

        // Setup on_track handler for incoming audio
        let producer = Arc::new(parking_lot::Mutex::new(Some(producer)));
        let stats_for_track = stats.clone();
        let track_sample_rate = sample_rate;
        let track_channels = channels;

        peer_connection.on_track(Box::new(
            move |track: Arc<TrackRemote>, _receiver: Arc<RTCRtpReceiver>, _transceiver: Arc<RTCRtpTransceiver>| {
                let codec = track.codec();

                // Only handle OPUS audio tracks
                if !codec.capability.mime_type.to_lowercase().contains("opus") {
                    return Box::pin(async {});
                }

                // Take the producer (only allow one incoming track)
                let producer = match producer.lock().take() {
                    Some(p) => p,
                    None => {
                        eprintln!("WHEP: on_track called but producer already taken");
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
            _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {
                // Continue even if ICE gathering not complete
            }
        }

        // Get the local description with gathered ICE candidates
        let local_desc = peer_connection
            .local_description()
            .await
            .ok_or("No local description available")?;

        // POST offer to WHEP endpoint
        let (answer_sdp, resource_url) = Self::post_offer(endpoint_url, &local_desc.sdp).await?;

        // Set remote description (answer)
        let answer = RTCSessionDescription::answer(answer_sdp)
            .map_err(|e| format!("Invalid answer SDP: {}", e))?;

        peer_connection
            .set_remote_description(answer)
            .await
            .map_err(|e| format!("Failed to set remote description: {}", e))?;

        Ok(Self {
            endpoint_url: endpoint_url.to_string(),
            peer_connection,
            incoming_consumer: Some(consumer),
            resource_url,
            connected: AtomicBool::new(false),
            stats,
        })
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

    /// Take the incoming audio consumer (for use by input stream)
    pub fn take_incoming_consumer(&mut self) -> Option<ringbuf::HeapCons<f32>> {
        self.incoming_consumer.take()
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

/// Spawn an async task to read RTP from a remote track and decode OPUS to PCM.
async fn spawn_track_reader(
    track: Arc<TrackRemote>,
    mut producer: ringbuf::HeapProd<f32>,
    stats: Arc<WhepClientStats>,
    sample_rate: u32,
    channels: u16,
) {
    // Create OPUS decoder for 20ms frames
    let format = AudioFormat::new(sample_rate, channels as u8);
    let mut decoder = match OpusDecoder::new(format, 20.0) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("WHEP: Failed to create OPUS decoder: {:?}", e);
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

                        // Push decoded samples to ring buffer (lock-free)
                        let pushed = producer.push_slice(&pcm_buffer[..total_samples]);

                        if pushed < total_samples {
                            stats.decode_errors.fetch_add(1, Ordering::Relaxed);
                        }

                        stats.packets_received.fetch_add(1, Ordering::Relaxed);
                        stats.bytes_received.fetch_add(payload.len() as u64, Ordering::Relaxed);
                    }
                    Err(e) => {
                        stats.decode_errors.fetch_add(1, Ordering::Relaxed);
                        eprintln!("WHEP: OPUS decode error: {:?}", e);
                    }
                }
            }
            Err(e) => {
                let err_str = e.to_string().to_lowercase();
                if err_str.contains("eof") || err_str.contains("closed") {
                    break;
                }
                eprintln!("WHEP: RTP read error: {}", e);
            }
        }
    }
}

impl Drop for WhepClient {
    fn drop(&mut self) {
        // Note: Can't do async cleanup in drop
        // Caller should call disconnect() before dropping
    }
}
