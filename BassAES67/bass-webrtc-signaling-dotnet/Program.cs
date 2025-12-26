// WebRTC Signaling Server Example with Room Support
//
// A simple example showing how to use the SignalingServer class.
// Messages are only relayed to clients in the same room.
//
// Usage:
//   dotnet run [port]
//
// Example:
//   dotnet run 8080

using BassWebRtc;

int port = args.Length > 0 && int.TryParse(args[0], out int p) ? p : 8080;

Console.WriteLine("==========================================");
Console.WriteLine("  WebRTC Signaling Server (C#)");
Console.WriteLine("  Room-Based Routing Enabled");
Console.WriteLine("==========================================");
Console.WriteLine();

using var cts = new CancellationTokenSource();

Console.CancelKeyPress += (_, e) =>
{
    e.Cancel = true;
    Console.WriteLine("\nReceived Ctrl+C, stopping...");
    cts.Cancel();
};

using var server = new SignalingServer(port);

server.OnClientConnected += (id, roomId) =>
{
    Console.WriteLine($"[Event] Client {id:N} connected to room '{roomId}'");
};

server.OnClientDisconnected += (id, roomId) =>
{
    Console.WriteLine($"[Event] Client {id:N} disconnected from room '{roomId}'");
};

server.OnMessageReceived += (id, roomId, msg) =>
{
    // Just log the message type, not the full content
    if (msg.Contains("\"type\":\"offer\""))
        Console.WriteLine($"[Relay] {id:N} ({roomId}) -> offer");
    else if (msg.Contains("\"type\":\"answer\""))
        Console.WriteLine($"[Relay] {id:N} ({roomId}) -> answer");
    else if (msg.Contains("\"type\":\"ice\""))
        Console.WriteLine($"[Relay] {id:N} ({roomId}) -> ice");
    else
        Console.WriteLine($"[Relay] {id:N} ({roomId}) -> {msg.Length} bytes");
};

Console.WriteLine($"WebSocket URL: ws://localhost:{port}/{{room_id}}");
Console.WriteLine();
Console.WriteLine("Instructions:");
Console.WriteLine("  1. Start this server");
Console.WriteLine("  2. Start the Rust WebRTC peer with --room <room_id>");
Console.WriteLine("  3. Open test_client_websocket.html in a browser");
Console.WriteLine("  4. Enter the same room ID and connect");
Console.WriteLine("  5. Audio will flow bidirectionally between browser and Rust");
Console.WriteLine();
Console.WriteLine("Example rooms:");
Console.WriteLine("  ws://localhost:" + port + "/studio-1");
Console.WriteLine("  ws://localhost:" + port + "/broadcast-abc");
Console.WriteLine();
Console.WriteLine("--- Running (Ctrl+C to stop) ---");
Console.WriteLine();

try
{
    await server.StartAsync(cts.Token);
}
catch (OperationCanceledException)
{
    // Normal shutdown
}

Console.WriteLine();
Console.WriteLine("Done!");
