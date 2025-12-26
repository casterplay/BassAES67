namespace BassWebRtc;

/// <summary>
/// WebSocket signaling server for WebRTC peer-to-peer connections (Rust implementation).
///
/// This is an OPTIONAL component - you can use either this Rust signaling server
/// or the C# SignalingServer from bass-webrtc-signaling-dotnet.
///
/// The signaling server is a pure WebSocket relay - it does NOT handle any
/// WebRTC logic. It simply relays JSON messages between connected clients
/// (browser and Rust WebRTC peer) within the same room.
///
/// Example:
///   using var sigServer = new BassWebRtcSignalingServer(8080);
///   sigServer.Start();
///   // Server is now listening on ws://localhost:8080
///   // Clients connect with room: ws://localhost:8080/{roomId}
/// </summary>
public class BassWebRtcSignalingServer : IDisposable
{
    private IntPtr _handle;
    private bool _disposed;

    /// <summary>
    /// Gets the port the server is listening on.
    /// </summary>
    public ushort Port { get; }

    /// <summary>
    /// Gets the number of currently connected WebSocket clients.
    /// </summary>
    public uint ClientCount => _handle != IntPtr.Zero
        ? BassWebRtcNative.BASS_WEBRTC_SignalingServerClientCount(_handle)
        : 0;

    /// <summary>
    /// Create a WebSocket signaling server.
    /// </summary>
    /// <param name="port">Port to listen on (e.g., 8080)</param>
    public BassWebRtcSignalingServer(ushort port)
    {
        Port = port;
        _handle = BassWebRtcNative.BASS_WEBRTC_CreateSignalingServer(port);
        if (_handle == IntPtr.Zero)
            throw new InvalidOperationException($"Failed to create signaling server: BASS error {BassWebRtcNative.BASS_ErrorGetCode()}");
    }

    /// <summary>
    /// Start the signaling server.
    /// This starts the WebSocket server in a background thread.
    /// </summary>
    /// <returns>True on success, false on failure</returns>
    public bool Start()
    {
        if (_handle == IntPtr.Zero) return false;
        return BassWebRtcNative.BASS_WEBRTC_SignalingServerStart(_handle) != 0;
    }

    /// <summary>
    /// Stop the signaling server.
    /// </summary>
    public void Stop()
    {
        if (_handle != IntPtr.Zero)
            BassWebRtcNative.BASS_WEBRTC_SignalingServerStop(_handle);
    }

    /// <summary>
    /// Dispose of the server and release all resources.
    /// </summary>
    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;

        if (_handle != IntPtr.Zero)
        {
            BassWebRtcNative.BASS_WEBRTC_SignalingServerFree(_handle);
            _handle = IntPtr.Zero;
        }

        GC.SuppressFinalize(this);
    }

    ~BassWebRtcSignalingServer() => Dispose();
}
