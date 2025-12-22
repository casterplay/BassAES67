using System.Collections.Concurrent;
using System.Net.WebSockets;

namespace BlazorServerApp.Services;

/// <summary>
/// WebSocket middleware for broadcasting Opus audio frames to connected clients.
/// Handles the /audio-ws endpoint for raw WebSocket connections.
/// </summary>
public class AudioWebSocketMiddleware
{
    private readonly RequestDelegate next;
    private readonly ILogger<AudioWebSocketMiddleware> logger;

    // Connected WebSocket clients
    private static readonly ConcurrentDictionary<string, WebSocket> clients = new();

    public AudioWebSocketMiddleware(RequestDelegate next, ILogger<AudioWebSocketMiddleware> logger)
    {
        this.next = next;
        this.logger = logger;
    }

    public async Task InvokeAsync(HttpContext context)
    {
        if (context.Request.Path == "/audio-ws")
        {
            if (context.WebSockets.IsWebSocketRequest)
            {
                var webSocket = await context.WebSockets.AcceptWebSocketAsync();
                var clientId = Guid.NewGuid().ToString();

                clients.TryAdd(clientId, webSocket);
                logger.LogInformation("WebSocket client connected: {ClientId} (total: {Count})", clientId, clients.Count);

                try
                {
                    await HandleWebSocketConnection(webSocket, clientId);
                }
                finally
                {
                    clients.TryRemove(clientId, out _);
                    logger.LogInformation("WebSocket client disconnected: {ClientId} (total: {Count})", clientId, clients.Count);
                }
            }
            else
            {
                context.Response.StatusCode = 400;
            }
        }
        else
        {
            await next(context);
        }
    }

    /// <summary>
    /// Handle a WebSocket connection - keep alive until client disconnects.
    /// </summary>
    private async Task HandleWebSocketConnection(WebSocket webSocket, string clientId)
    {
        var buffer = new byte[1024];

        while (webSocket.State == WebSocketState.Open)
        {
            try
            {
                var result = await webSocket.ReceiveAsync(new ArraySegment<byte>(buffer), CancellationToken.None);

                if (result.MessageType == WebSocketMessageType.Close)
                {
                    await webSocket.CloseAsync(WebSocketCloseStatus.NormalClosure, "Closed by client", CancellationToken.None);
                    break;
                }
            }
            catch (WebSocketException)
            {
                break;
            }
        }
    }

    /// <summary>
    /// Broadcast an Opus frame to all connected clients.
    /// Called from the AudioEncoderService callback.
    /// </summary>
    public static async Task BroadcastFrameAsync(byte[] data)
    {
        if (clients.IsEmpty)
            return;

        var disconnected = new List<string>();

        foreach (var (clientId, webSocket) in clients)
        {
            if (webSocket.State == WebSocketState.Open)
            {
                try
                {
                    await webSocket.SendAsync(
                        new ArraySegment<byte>(data),
                        WebSocketMessageType.Binary,
                        true,
                        CancellationToken.None
                    );
                }
                catch
                {
                    disconnected.Add(clientId);
                }
            }
            else
            {
                disconnected.Add(clientId);
            }
        }

        // Clean up disconnected clients
        foreach (var clientId in disconnected)
        {
            clients.TryRemove(clientId, out _);
        }
    }

    /// <summary>
    /// Broadcast an Opus frame synchronously (for use in callback).
    /// Fire-and-forget to avoid blocking the encoder thread.
    /// </summary>
    public static void BroadcastFrame(byte[] data)
    {
        _ = BroadcastFrameAsync(data);
    }

    /// <summary>
    /// Get the number of connected clients.
    /// </summary>
    public static int ClientCount => clients.Count;
}

/// <summary>
/// Extension methods for registering the WebSocket middleware.
/// </summary>
public static class AudioWebSocketMiddlewareExtensions
{
    public static IApplicationBuilder UseAudioWebSocket(this IApplicationBuilder app)
    {
        return app.UseMiddleware<AudioWebSocketMiddleware>();
    }
}
