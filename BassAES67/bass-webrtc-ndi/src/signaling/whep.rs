//! WHEP (WebRTC-HTTP Egress Protocol) signaling.
//!
//! Similar to WHIP but for egress (browser pulls from server).
//! The browser sends an SDP offer, we respond with an answer.

use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use parking_lot::Mutex;
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use crate::peer::PeerManager;

/// WHEP endpoint configuration
pub struct WhepConfig {
    /// Port to listen on
    pub port: u16,
    /// Bind address (default 0.0.0.0)
    pub bind_addr: String,
    /// Path prefix (default "/whep")
    pub path_prefix: String,
}

impl Default for WhepConfig {
    fn default() -> Self {
        Self {
            port: 8081,
            bind_addr: "0.0.0.0".to_string(),
            path_prefix: "/whep".to_string(),
        }
    }
}

/// WHEP endpoint handler
pub struct WhepEndpoint {
    config: WhepConfig,
    peer_manager: Arc<Mutex<PeerManager>>,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl WhepEndpoint {
    /// Create a new WHEP endpoint
    pub fn new(config: WhepConfig, peer_manager: Arc<Mutex<PeerManager>>) -> Self {
        Self {
            config,
            peer_manager,
            shutdown_tx: None,
        }
    }

    /// Start the WHEP HTTP server
    pub async fn start(&mut self) -> Result<(), String> {
        let addr: SocketAddr = format!("{}:{}", self.config.bind_addr, self.config.port)
            .parse()
            .map_err(|e| format!("Invalid address: {}", e))?;

        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| format!("Failed to bind: {}", e))?;

        let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();
        self.shutdown_tx = Some(shutdown_tx);

        let peer_manager = self.peer_manager.clone();
        let path_prefix = self.config.path_prefix.clone();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    result = listener.accept() => {
                        match result {
                            Ok((stream, _)) => {
                                let io = TokioIo::new(stream);
                                let pm = peer_manager.clone();
                                let prefix = path_prefix.clone();

                                tokio::spawn(async move {
                                    let service = service_fn(move |req| {
                                        let pm = pm.clone();
                                        let prefix = prefix.clone();
                                        async move {
                                            handle_request(req, pm, &prefix).await
                                        }
                                    });

                                    if let Err(e) = http1::Builder::new()
                                        .serve_connection(io, service)
                                        .await
                                    {
                                        eprintln!("WHEP connection error: {}", e);
                                    }
                                });
                            }
                            Err(e) => {
                                eprintln!("WHEP accept error: {}", e);
                            }
                        }
                    }
                    _ = &mut shutdown_rx => {
                        break;
                    }
                }
            }
        });

        Ok(())
    }

    /// Stop the WHEP HTTP server
    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

/// Handle incoming HTTP request
async fn handle_request(
    req: Request<Incoming>,
    peer_manager: Arc<Mutex<PeerManager>>,
    path_prefix: &str,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let path = req.uri().path();
    let method = req.method();

    // Check path prefix
    if !path.starts_with(path_prefix) {
        return Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Full::new(Bytes::from("Not Found")))
            .unwrap());
    }

    let sub_path = &path[path_prefix.len()..];

    match (method, sub_path) {
        // POST /whep - New egress connection with SDP offer from browser
        (&Method::POST, "" | "/") => {
            handle_post_offer(req, peer_manager).await
        }

        // PATCH /whep/{id} - Trickle ICE candidate
        (&Method::PATCH, id_path) if id_path.starts_with('/') => {
            let peer_id: u32 = id_path[1..].parse().unwrap_or(u32::MAX);
            handle_patch_ice(req, peer_manager, peer_id).await
        }

        // DELETE /whep/{id} - Close connection
        (&Method::DELETE, id_path) if id_path.starts_with('/') => {
            let peer_id: u32 = id_path[1..].parse().unwrap_or(u32::MAX);
            handle_delete(peer_manager, peer_id).await
        }

        // OPTIONS - CORS preflight
        (&Method::OPTIONS, _) => {
            Ok(Response::builder()
                .status(StatusCode::NO_CONTENT)
                .header("Access-Control-Allow-Origin", "*")
                .header("Access-Control-Allow-Methods", "POST, PATCH, DELETE, OPTIONS")
                .header("Access-Control-Allow-Headers", "Content-Type")
                .body(Full::new(Bytes::new()))
                .unwrap())
        }

        _ => {
            Ok(Response::builder()
                .status(StatusCode::METHOD_NOT_ALLOWED)
                .body(Full::new(Bytes::from("Method Not Allowed")))
                .unwrap())
        }
    }
}

/// Handle POST request - new egress connection
async fn handle_post_offer(
    req: Request<Incoming>,
    peer_manager: Arc<Mutex<PeerManager>>,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    // Read body (SDP offer from browser)
    let body_bytes = req.collect().await?.to_bytes();
    let offer_sdp = match String::from_utf8(body_bytes.to_vec()) {
        Ok(s) => s,
        Err(_) => {
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Full::new(Bytes::from("Invalid UTF-8 in body")))
                .unwrap());
        }
    };

    // Add peer with offer (same as WHIP - browser sends offer, we respond with answer)
    let result = {
        let mut pm = peer_manager.lock();
        tokio::runtime::Handle::current().block_on(pm.add_peer_with_offer(&offer_sdp))
    };

    match result {
        Ok((peer_id, answer_sdp)) => {
            Ok(Response::builder()
                .status(StatusCode::CREATED)
                .header("Content-Type", "application/sdp")
                .header("Location", format!("/whep/{}", peer_id))
                .header("Access-Control-Allow-Origin", "*")
                .header("Access-Control-Expose-Headers", "Location")
                .body(Full::new(Bytes::from(answer_sdp)))
                .unwrap())
        }
        Err(e) => {
            let status = if e.contains("Maximum peers") {
                StatusCode::SERVICE_UNAVAILABLE
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };

            Ok(Response::builder()
                .status(status)
                .header("Access-Control-Allow-Origin", "*")
                .body(Full::new(Bytes::from(e)))
                .unwrap())
        }
    }
}

/// Handle PATCH request - trickle ICE candidate
async fn handle_patch_ice(
    req: Request<Incoming>,
    peer_manager: Arc<Mutex<PeerManager>>,
    peer_id: u32,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let body_bytes = req.collect().await?.to_bytes();
    let candidate_str = match String::from_utf8(body_bytes.to_vec()) {
        Ok(s) => s,
        Err(_) => {
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Full::new(Bytes::from("Invalid UTF-8 in body")))
                .unwrap());
        }
    };

    let candidate = if candidate_str.starts_with("a=") {
        candidate_str.trim_start_matches("a=").to_string()
    } else {
        candidate_str
    };

    let result = {
        let pm = peer_manager.lock();
        tokio::runtime::Handle::current().block_on(
            pm.add_ice_candidate(peer_id, &candidate, None, None)
        )
    };

    match result {
        Ok(()) => {
            Ok(Response::builder()
                .status(StatusCode::NO_CONTENT)
                .header("Access-Control-Allow-Origin", "*")
                .body(Full::new(Bytes::new()))
                .unwrap())
        }
        Err(e) => {
            Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header("Access-Control-Allow-Origin", "*")
                .body(Full::new(Bytes::from(e)))
                .unwrap())
        }
    }
}

/// Handle DELETE request - close connection
async fn handle_delete(
    peer_manager: Arc<Mutex<PeerManager>>,
    peer_id: u32,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let result = {
        let mut pm = peer_manager.lock();
        tokio::runtime::Handle::current().block_on(pm.remove_peer(peer_id))
    };

    match result {
        Ok(()) => {
            Ok(Response::builder()
                .status(StatusCode::NO_CONTENT)
                .header("Access-Control-Allow-Origin", "*")
                .body(Full::new(Bytes::new()))
                .unwrap())
        }
        Err(e) => {
            Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header("Access-Control-Allow-Origin", "*")
                .body(Full::new(Bytes::from(e)))
                .unwrap())
        }
    }
}
