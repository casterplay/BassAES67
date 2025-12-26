namespace BassWebRtc;

/// <summary>
/// WebRTC server with built-in WHIP/WHEP signaling endpoints (OPTIONAL).
///
/// This is an OPTIONAL component for when you want bass-webrtc to host its own
/// HTTP-based signaling endpoints. Supports up to 5 simultaneous browser connections.
///
/// For most use cases, prefer using BassWebRtcPeer with an external signaling server
/// (either Rust BassWebRtcSignalingServer or C# SignalingServer).
///
/// Example:
///   var config = WebRtcConfigFFI.CreateDefault();
///   config.SignalingMode = BassWebRtcNative.BASS_WEBRTC_SIGNALING_WHIP;
///   config.HttpPort = 8080;
///   using var server = new BassWebRtcServer(bassChannel, config);
///   server.Start();
///   // Browsers can now POST to http://localhost:8080/whip
///   int inputStream = server.InputStreamHandle;
/// </summary>
public class BassWebRtcServer : IDisposable
{
    private IntPtr _handle;
    private bool _disposed;

    /// <summary>
    /// Gets the BASS stream handle for audio received from browsers.
    /// Use this to play the audio or add it to a mixer.
    /// </summary>
    public int InputStreamHandle => _handle != IntPtr.Zero
        ? BassWebRtcNative.BASS_WEBRTC_GetInputStream(_handle)
        : 0;

    /// <summary>
    /// Gets whether the server is running.
    /// </summary>
    public bool IsRunning => _handle != IntPtr.Zero && BassWebRtcNative.BASS_WEBRTC_IsRunning(_handle) != 0;

    /// <summary>
    /// Gets the number of active peer connections.
    /// </summary>
    public uint PeerCount => _handle != IntPtr.Zero
        ? BassWebRtcNative.BASS_WEBRTC_GetPeerCount(_handle)
        : 0;

    /// <summary>
    /// Create a WebRTC server.
    /// </summary>
    /// <param name="sourceChannel">BASS channel to read audio from (for output to browsers)</param>
    /// <param name="config">Server configuration</param>
    public BassWebRtcServer(int sourceChannel, BassWebRtcNative.WebRtcConfigFFI config)
    {
        _handle = BassWebRtcNative.BASS_WEBRTC_Create(sourceChannel, ref config);
        if (_handle == IntPtr.Zero)
            throw new InvalidOperationException($"Failed to create WebRTC server: BASS error {BassWebRtcNative.BASS_ErrorGetCode()}");
    }

    /// <summary>
    /// Start the WebRTC server.
    /// </summary>
    /// <returns>True on success, false on failure</returns>
    public bool Start()
    {
        if (_handle == IntPtr.Zero) return false;
        return BassWebRtcNative.BASS_WEBRTC_Start(_handle) != 0;
    }

    /// <summary>
    /// Stop the WebRTC server.
    /// </summary>
    public void Stop()
    {
        if (_handle != IntPtr.Zero)
            BassWebRtcNative.BASS_WEBRTC_Stop(_handle);
    }

    /// <summary>
    /// Add an ICE server (STUN or TURN) for NAT traversal.
    /// Call this before Start() for best results.
    /// </summary>
    /// <param name="url">Server URL (e.g., "stun:stun.l.google.com:19302" or "turn:server:3478")</param>
    /// <param name="username">Username for TURN (null for STUN)</param>
    /// <param name="credential">Credential for TURN (null for STUN)</param>
    public void AddIceServer(string url, string? username = null, string? credential = null)
    {
        if (_handle != IntPtr.Zero)
            BassWebRtcNative.BASS_WEBRTC_AddIceServer(_handle, url, username, credential);
    }

    /// <summary>
    /// Get current statistics.
    /// </summary>
    /// <returns>Statistics structure, or default if not running</returns>
    public BassWebRtcNative.WebRtcStatsFFI GetStats()
    {
        if (_handle != IntPtr.Zero && BassWebRtcNative.BASS_WEBRTC_GetStats(_handle, out var stats) != 0)
            return stats;
        return default;
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
            BassWebRtcNative.BASS_WEBRTC_Free(_handle);
            _handle = IntPtr.Zero;
        }

        GC.SuppressFinalize(this);
    }

    ~BassWebRtcServer() => Dispose();
}
