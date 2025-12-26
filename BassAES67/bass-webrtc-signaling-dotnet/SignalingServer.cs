// WebSocket Signaling Server for WebRTC with Room Support
//
// A pure WebSocket message relay for WebRTC signaling with room-based routing.
// This server does NOT handle any WebRTC logic - it simply relays
// JSON messages between connected clients (browser and Rust WebRTC peer)
// that are in the SAME ROOM.
//
// Room-based routing:
// - Room ID is extracted from the URL path: ws://server:port/{room_id}
// - Messages are only relayed to other clients in the same room
// - Empty room ID defaults to "default" room
//
// Message flow:
// 1. Browser connects to ws://server:8080/my-room
// 2. Rust peer connects to ws://server:8080/my-room
// 3. Browser sends offer SDP -> Server relays ONLY to clients in "my-room"
// 4. Rust peer sends answer SDP -> Server relays ONLY to clients in "my-room"
// 5. ICE candidates are relayed within the same room
// 6. After ICE completes, audio flows directly peer-to-peer
//
// Usage:
//   var server = new SignalingServer(8080);
//   await server.StartAsync();

using System;
using System.Collections.Concurrent;
using System.Net;
using System.Net.WebSockets;
using System.Text;
using System.Threading;
using System.Threading.Tasks;

namespace BassWebRtc;

/// <summary>
/// Represents a client connected to a specific room.
/// </summary>
internal class RoomClient
{
    public Guid ClientId { get; }
    public WebSocket WebSocket { get; }
    public string RoomId { get; }

    public RoomClient(Guid clientId, WebSocket webSocket, string roomId)
    {
        ClientId = clientId;
        WebSocket = webSocket;
        RoomId = roomId;
    }
}

/// <summary>
/// WebSocket signaling server that relays SDP/ICE messages between WebRTC peers.
/// Messages are only relayed to clients in the same room.
/// No WebRTC library needed - just pure message passing.
/// </summary>
public class SignalingServer : IDisposable
{
    private readonly int _port;
    // room_id -> (client_id -> RoomClient)
    private readonly ConcurrentDictionary<string, ConcurrentDictionary<Guid, RoomClient>> _rooms;
    private HttpListener? _listener;
    private CancellationTokenSource? _cts;
    private bool _isRunning;

    /// <summary>
    /// Event raised when a message is received (for logging/debugging).
    /// </summary>
    public event Action<Guid, string, string>? OnMessageReceived; // clientId, roomId, message

    /// <summary>
    /// Event raised when a client connects.
    /// </summary>
    public event Action<Guid, string>? OnClientConnected; // clientId, roomId

    /// <summary>
    /// Event raised when a client disconnects.
    /// </summary>
    public event Action<Guid, string>? OnClientDisconnected; // clientId, roomId

    /// <summary>
    /// Creates a new signaling server on the specified port.
    /// </summary>
    /// <param name="port">Port to listen on (e.g., 8080)</param>
    public SignalingServer(int port)
    {
        _port = port;
        _rooms = new ConcurrentDictionary<string, ConcurrentDictionary<Guid, RoomClient>>();
    }

    /// <summary>
    /// Gets the port this server is configured for.
    /// </summary>
    public int Port => _port;

    /// <summary>
    /// Gets whether the server is currently running.
    /// </summary>
    public bool IsRunning => _isRunning;

    /// <summary>
    /// Gets the total number of connected clients across all rooms.
    /// </summary>
    public int ClientCount
    {
        get
        {
            int count = 0;
            foreach (var room in _rooms.Values)
            {
                count += room.Count;
            }
            return count;
        }
    }

    /// <summary>
    /// Gets the number of active rooms.
    /// </summary>
    public int RoomCount => _rooms.Count;

    /// <summary>
    /// Gets the number of clients in a specific room.
    /// </summary>
    public int GetRoomClientCount(string roomId)
    {
        return _rooms.TryGetValue(roomId, out var room) ? room.Count : 0;
    }

    /// <summary>
    /// Starts the signaling server.
    /// </summary>
    public async Task StartAsync(CancellationToken cancellationToken = default)
    {
        if (_isRunning)
            throw new InvalidOperationException("Server is already running");

        _cts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);

        _listener = new HttpListener();
        _listener.Prefixes.Add($"http://+:{_port}/");

        try
        {
            _listener.Start();
        }
        catch (HttpListenerException)
        {
            // Try localhost only if we can't bind to all interfaces
            _listener = new HttpListener();
            _listener.Prefixes.Add($"http://localhost:{_port}/");
            _listener.Start();
        }

        _isRunning = true;
        Console.WriteLine($"[SignalingServer] Listening on ws://localhost:{_port}/{{room_id}}");

        while (!_cts.Token.IsCancellationRequested)
        {
            try
            {
                var context = await _listener.GetContextAsync().ConfigureAwait(false);

                if (context.Request.IsWebSocketRequest)
                {
                    _ = HandleWebSocketAsync(context, _cts.Token);
                }
                else
                {
                    // Return a simple info page for HTTP requests
                    context.Response.StatusCode = 200;
                    context.Response.ContentType = "text/html";
                    var body = Encoding.UTF8.GetBytes(
                        "<h1>WebRTC Signaling Server</h1>" +
                        "<p>Connect via WebSocket to: ws://localhost:" + _port + "/{room_id}</p>" +
                        $"<p>Active rooms: {_rooms.Count}</p>" +
                        $"<p>Total connected clients: {ClientCount}</p>");
                    await context.Response.OutputStream.WriteAsync(body, _cts.Token);
                    context.Response.Close();
                }
            }
            catch (OperationCanceledException)
            {
                break;
            }
            catch (HttpListenerException) when (_cts.Token.IsCancellationRequested)
            {
                break;
            }
            catch (Exception ex)
            {
                Console.WriteLine($"[SignalingServer] Error: {ex.Message}");
            }
        }

        _isRunning = false;
        Console.WriteLine("[SignalingServer] Stopped");
    }

    /// <summary>
    /// Stops the signaling server.
    /// </summary>
    public void Stop()
    {
        _cts?.Cancel();
        _listener?.Stop();
        _listener?.Close();

        // Close all client connections in all rooms
        foreach (var room in _rooms.Values)
        {
            foreach (var client in room.Values)
            {
                try
                {
                    client.WebSocket.CloseAsync(WebSocketCloseStatus.NormalClosure, "Server stopping", CancellationToken.None)
                        .Wait(TimeSpan.FromSeconds(1));
                }
                catch { }
            }
        }

        _rooms.Clear();
    }

    private async Task HandleWebSocketAsync(HttpListenerContext context, CancellationToken ct)
    {
        var clientId = Guid.NewGuid();
        WebSocket? ws = null;

        // Extract room ID from URL path
        var path = context.Request.Url?.AbsolutePath ?? "/";
        var roomId = path.TrimStart('/');
        if (string.IsNullOrEmpty(roomId))
        {
            roomId = "default";
        }

        try
        {
            var wsContext = await context.AcceptWebSocketAsync(null).ConfigureAwait(false);
            ws = wsContext.WebSocket;

            // Get or create the room
            var room = _rooms.GetOrAdd(roomId, _ => new ConcurrentDictionary<Guid, RoomClient>());
            var roomClient = new RoomClient(clientId, ws, roomId);
            room[clientId] = roomClient;

            Console.WriteLine($"[SignalingServer] Client {clientId:N} connected to room '{roomId}', room now has {room.Count} client(s)");
            OnClientConnected?.Invoke(clientId, roomId);

            var buffer = new byte[4096];

            while (ws.State == WebSocketState.Open && !ct.IsCancellationRequested)
            {
                var result = await ws.ReceiveAsync(new ArraySegment<byte>(buffer), ct).ConfigureAwait(false);

                if (result.MessageType == WebSocketMessageType.Close)
                {
                    break;
                }

                if (result.MessageType == WebSocketMessageType.Text)
                {
                    var message = Encoding.UTF8.GetString(buffer, 0, result.Count);
                    OnMessageReceived?.Invoke(clientId, roomId, message);

                    // Relay message to all OTHER connected clients in the SAME ROOM
                    await RelayMessageToRoomAsync(roomId, clientId, message, ct).ConfigureAwait(false);
                }
            }
        }
        catch (OperationCanceledException)
        {
            // Normal shutdown
        }
        catch (WebSocketException ex) when (ex.WebSocketErrorCode == WebSocketError.ConnectionClosedPrematurely)
        {
            // Client disconnected abruptly
        }
        catch (Exception ex)
        {
            Console.WriteLine($"[SignalingServer] Client {clientId:N} error: {ex.Message}");
        }
        finally
        {
            // Remove client from room
            if (_rooms.TryGetValue(roomId, out var room))
            {
                room.TryRemove(clientId, out _);
                var remaining = room.Count;
                Console.WriteLine($"[SignalingServer] Client {clientId:N} left room '{roomId}', {remaining} client(s) remaining");

                // Remove empty rooms
                if (remaining == 0)
                {
                    _rooms.TryRemove(roomId, out _);
                    Console.WriteLine($"[SignalingServer] Room '{roomId}' deleted (empty)");
                }
            }

            OnClientDisconnected?.Invoke(clientId, roomId);

            if (ws != null && ws.State == WebSocketState.Open)
            {
                try
                {
                    await ws.CloseAsync(WebSocketCloseStatus.NormalClosure, "Goodbye", CancellationToken.None)
                        .ConfigureAwait(false);
                }
                catch { }
            }

            ws?.Dispose();
        }
    }

    private async Task RelayMessageToRoomAsync(string roomId, Guid senderId, string message, CancellationToken ct)
    {
        if (!_rooms.TryGetValue(roomId, out var room))
            return;

        var messageBytes = Encoding.UTF8.GetBytes(message);
        var segment = new ArraySegment<byte>(messageBytes);

        foreach (var kvp in room)
        {
            if (kvp.Key == senderId)
                continue; // Don't send back to sender

            if (kvp.Value.WebSocket.State != WebSocketState.Open)
                continue;

            try
            {
                await kvp.Value.WebSocket.SendAsync(segment, WebSocketMessageType.Text, true, ct).ConfigureAwait(false);
            }
            catch
            {
                // Client may have disconnected
            }
        }
    }

    public void Dispose()
    {
        Stop();
        _cts?.Dispose();
    }
}
