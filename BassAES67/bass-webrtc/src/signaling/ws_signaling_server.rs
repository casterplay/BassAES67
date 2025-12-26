//! WebSocket Signaling Server with Room Support
//!
//! A pure WebSocket message relay for WebRTC signaling with room-based routing.
//! This server does NOT handle any WebRTC logic - it simply relays
//! JSON messages between connected clients (browser and Rust WebRTC peer)
//! that are in the SAME ROOM.
//!
//! Room-based routing:
//! - Room ID is extracted from the URL path: ws://server:port/{room_id}
//! - Messages are only relayed to other clients in the same room
//! - Empty room ID defaults to "default" room
//!
//! Message flow:
//! 1. Browser connects to ws://server:8080/my-room
//! 2. Rust peer connects to ws://server:8080/my-room
//! 3. Browser sends offer SDP -> Server relays ONLY to clients in "my-room"
//! 4. Rust peer sends answer SDP -> Server relays ONLY to clients in "my-room"
//! 5. ICE candidates are relayed within the same room
//! 6. After ICE completes, audio flows directly peer-to-peer

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

/// Unique client ID
type ClientId = u64;

/// Room ID (string identifier)
type RoomId = String;

/// Message sender for a connected client
type ClientSender = mpsc::UnboundedSender<Message>;

/// Room containing connected clients
struct Room {
    clients: HashMap<ClientId, ClientSender>,
}

impl Room {
    fn new() -> Self {
        Self {
            clients: HashMap::new(),
        }
    }
}

/// Signaling server that relays WebSocket messages between clients in the same room
pub struct SignalingServer {
    port: u16,
    /// Rooms mapped by room ID, each containing clients
    rooms: Arc<Mutex<HashMap<RoomId, Room>>>,
    next_client_id: AtomicU64,
    running: Arc<std::sync::atomic::AtomicBool>,
}

impl SignalingServer {
    /// Create a new signaling server on the specified port
    pub fn new(port: u16) -> Self {
        Self {
            port,
            rooms: Arc::new(Mutex::new(HashMap::new())),
            next_client_id: AtomicU64::new(1),
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Get the port this server is configured for
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Check if the server is running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Get the total number of connected clients across all rooms
    pub fn client_count(&self) -> usize {
        let rooms = self.rooms.lock();
        rooms.values().map(|r| r.clients.len()).sum()
    }

    /// Get the number of active rooms
    pub fn room_count(&self) -> usize {
        self.rooms.lock().len()
    }

    /// Stop the server
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Run the signaling server (blocking)
    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let addr = format!("0.0.0.0:{}", self.port);
        let listener = TcpListener::bind(&addr).await?;

        self.running.store(true, Ordering::SeqCst);
        println!("[SignalingServer] Listening on ws://{}", addr);

        while self.running.load(Ordering::SeqCst) {
            // Use timeout to allow checking running flag
            let accept_result = tokio::time::timeout(
                std::time::Duration::from_millis(100),
                listener.accept()
            ).await;

            match accept_result {
                Ok(Ok((stream, addr))) => {
                    let client_id = self.next_client_id.fetch_add(1, Ordering::SeqCst);
                    let rooms = self.rooms.clone();

                    tokio::spawn(async move {
                        if let Err(e) = handle_connection(stream, addr, client_id, rooms).await {
                            eprintln!("[SignalingServer] Client {} error: {}", client_id, e);
                        }
                    });
                }
                Ok(Err(e)) => {
                    eprintln!("[SignalingServer] Accept error: {}", e);
                }
                Err(_) => {
                    // Timeout - just continue to check running flag
                }
            }
        }

        println!("[SignalingServer] Stopped");
        Ok(())
    }
}

/// Handle a single WebSocket connection with room support
async fn handle_connection(
    stream: TcpStream,
    addr: SocketAddr,
    client_id: ClientId,
    rooms: Arc<Mutex<HashMap<RoomId, Room>>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Use custom handshake callback to extract room ID from URL path
    let mut room_id = String::from("default");

    let ws_stream = tokio_tungstenite::accept_hdr_async(
        stream,
        |request: &tokio_tungstenite::tungstenite::handshake::server::Request,
         response: tokio_tungstenite::tungstenite::handshake::server::Response| {
            // Extract room ID from URL path
            let path = request.uri().path();
            // Remove leading slash and use as room ID
            let extracted_room = path.trim_start_matches('/');
            if !extracted_room.is_empty() {
                room_id = extracted_room.to_string();
            }
            Ok(response)
        },
    ).await?;

    println!("[SignalingServer] Client {} connected from {} to room '{}'",
             client_id, addr, room_id);

    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

    // Create channel for sending messages to this client
    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();

    // Register this client in its room
    {
        let mut rooms_guard = rooms.lock();
        let room = rooms_guard.entry(room_id.clone()).or_insert_with(Room::new);
        room.clients.insert(client_id, tx);
        println!("[SignalingServer] Room '{}' now has {} client(s)",
                 room_id, room.clients.len());
    }

    // Clone room_id for use in async tasks
    let room_id_for_relay = room_id.clone();
    let rooms_for_relay = rooms.clone();

    // Spawn task to forward messages from channel to WebSocket
    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if ws_sender.send(msg).await.is_err() {
                break;
            }
        }
    });

    // Process incoming messages and relay to other clients in the SAME ROOM
    while let Some(msg_result) = ws_receiver.next().await {
        match msg_result {
            Ok(msg) => {
                match &msg {
                    Message::Text(text) => {
                        // Relay text message to other clients in the SAME ROOM
                        relay_to_room(&rooms_for_relay, &room_id_for_relay, client_id,
                                      Message::Text(text.clone()));
                    }
                    Message::Binary(data) => {
                        // Relay binary message to other clients in the SAME ROOM
                        relay_to_room(&rooms_for_relay, &room_id_for_relay, client_id,
                                      Message::Binary(data.clone()));
                    }
                    Message::Ping(data) => {
                        // Respond with pong (handled automatically by tungstenite)
                        let _ = data; // Just to silence unused warning
                    }
                    Message::Pong(_) => {
                        // Ignore pong
                    }
                    Message::Close(_) => {
                        break;
                    }
                    Message::Frame(_) => {
                        // Raw frame - ignore
                    }
                }
            }
            Err(e) => {
                eprintln!("[SignalingServer] Client {} receive error: {}", client_id, e);
                break;
            }
        }
    }

    // Unregister client from its room
    {
        let mut rooms_guard = rooms.lock();
        if let Some(room) = rooms_guard.get_mut(&room_id) {
            room.clients.remove(&client_id);
            let remaining = room.clients.len();
            println!("[SignalingServer] Client {} left room '{}', {} client(s) remaining",
                     client_id, room_id, remaining);

            // Remove empty rooms
            if remaining == 0 {
                rooms_guard.remove(&room_id);
                println!("[SignalingServer] Room '{}' deleted (empty)", room_id);
            }
        }
    }

    send_task.abort();
    Ok(())
}

/// Relay a message to all clients in the same room except the sender
fn relay_to_room(
    rooms: &Arc<Mutex<HashMap<RoomId, Room>>>,
    room_id: &str,
    sender_id: ClientId,
    message: Message,
) {
    let rooms_guard = rooms.lock();
    if let Some(room) = rooms_guard.get(room_id) {
        for (&id, sender) in room.clients.iter() {
            if id != sender_id {
                let _ = sender.send(message.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_creation() {
        let server = SignalingServer::new(8080);
        assert_eq!(server.port(), 8080);
        assert!(!server.is_running());
        assert_eq!(server.client_count(), 0);
        assert_eq!(server.room_count(), 0);
    }
}
