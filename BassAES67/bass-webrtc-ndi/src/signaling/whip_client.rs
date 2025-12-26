//! WHIP Client - WebRTC-HTTP Ingestion Protocol client (RFC 9725).
//!
//! Connects to an external WHIP server (like MediaMTX) to push audio.
//! Flow:
//! 1. Create RTCPeerConnection with send-only audio track
//! 2. Create SDP offer
//! 3. POST offer to WHIP endpoint
//! 4. Set remote answer from response
//! 5. Stream audio via WebRTC

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{Method, Request, StatusCode};
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::APIBuilder;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;
use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;
use webrtc::track::track_local::TrackLocal;

use crate::peer::IceServerConfig;

/// WHIP client statistics
pub struct WhipClientStats {
    pub packets_sent: AtomicU64,
    pub bytes_sent: AtomicU64,
    pub encode_errors: AtomicU64,
}

impl Default for WhipClientStats {
    fn default() -> Self {
        Self {
            packets_sent: AtomicU64::new(0),
            bytes_sent: AtomicU64::new(0),
            encode_errors: AtomicU64::new(0),
        }
    }
}

/// WHIP client for pushing audio to an external WHIP server
pub struct WhipClient {
    /// WHIP endpoint URL
    endpoint_url: String,
    /// WebRTC peer connection
    peer_connection: Arc<RTCPeerConnection>,
    /// Outgoing audio track
    audio_track: Arc<TrackLocalStaticSample>,
    /// Resource URL returned by server (for DELETE on close)
    resource_url: Option<String>,
    /// Connection state
    connected: AtomicBool,
    /// Statistics
    pub stats: Arc<WhipClientStats>,
}

impl WhipClient {
    /// Create and connect a new WHIP client.
    ///
    /// # Arguments
    /// * `endpoint_url` - WHIP endpoint URL (e.g., "http://localhost:8889/mystream/whip")
    /// * `ice_servers` - List of ICE servers for NAT traversal
    /// * `sample_rate` - Audio sample rate (48000)
    /// * `channels` - Number of channels (1 or 2)
    pub async fn connect(
        endpoint_url: &str,
        ice_servers: &[IceServerConfig],
        sample_rate: u32,
        channels: u16,
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

        // Create audio track (OPUS at specified rate)
        let audio_track = Arc::new(TrackLocalStaticSample::new(
            RTCRtpCodecCapability {
                mime_type: "audio/opus".to_owned(),
                clock_rate: sample_rate,
                channels: channels,
                sdp_fmtp_line: "minptime=10;useinbandfec=1".to_owned(),
                rtcp_feedback: vec![],
            },
            "audio".to_owned(),
            "bass-webrtc-whip".to_owned(),
        ));

        // Add track to peer connection (send only)
        peer_connection
            .add_track(audio_track.clone() as Arc<dyn TrackLocal + Send + Sync>)
            .await
            .map_err(|e| format!("Failed to add audio track: {}", e))?;

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
                // Continue even if ICE gathering not complete (trickle ICE fallback)
            }
        }

        // Get the local description with gathered ICE candidates
        let local_desc = peer_connection
            .local_description()
            .await
            .ok_or("No local description available")?;

        // POST offer to WHIP endpoint
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
            audio_track,
            resource_url,
            connected: AtomicBool::new(false),
            stats: Arc::new(WhipClientStats::default()),
        })
    }

    /// POST SDP offer to WHIP endpoint
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
            .map_err(|e| format!("WHIP request failed: {}", e))?;

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
            return Err(format!("WHIP server returned {}: {}", status, body_str));
        }

        Ok((body_str, resource_url))
    }

    /// Check if connected
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    /// Get the audio track for writing samples
    pub fn audio_track(&self) -> &Arc<TrackLocalStaticSample> {
        &self.audio_track
    }

    /// Disconnect from the WHIP server
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

impl Drop for WhipClient {
    fn drop(&mut self) {
        // Note: Can't do async cleanup in drop
        // Caller should call disconnect() before dropping
    }
}
