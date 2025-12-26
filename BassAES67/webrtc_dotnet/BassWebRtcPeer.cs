using System.Runtime.InteropServices;

namespace BassWebRtc;

/// <summary>
/// WebRTC connection statistics including RTT, packet loss, and jitter.
/// </summary>
public class WebRtcStats
{
    /// <summary>Total packets sent to the browser.</summary>
    public ulong PacketsSent { get; init; }

    /// <summary>Total packets received from the browser.</summary>
    public ulong PacketsReceived { get; init; }

    /// <summary>Total bytes sent to the browser.</summary>
    public ulong BytesSent { get; init; }

    /// <summary>Total bytes received from the browser.</summary>
    public ulong BytesReceived { get; init; }

    /// <summary>Round-trip time (latency) to the browser.</summary>
    public TimeSpan RoundTripTime { get; init; }

    /// <summary>Total packets lost (negative = duplicates received).</summary>
    public long PacketsLost { get; init; }

    /// <summary>Packet loss percentage (0.0 - 100.0).</summary>
    public float PacketLossPercent { get; init; }

    /// <summary>Jitter (variation in packet arrival time).</summary>
    public TimeSpan Jitter { get; init; }

    /// <summary>Number of NACK (Negative Acknowledgement) requests.</summary>
    public ulong NackCount { get; init; }

    /// <summary>Calculated send bitrate in kilobits per second.</summary>
    public double SendBitrateKbps { get; internal set; }

    /// <summary>Calculated receive bitrate in kilobits per second.</summary>
    public double ReceiveBitrateKbps { get; internal set; }
}

/// <summary>
/// WebRTC peer for bidirectional audio via WebSocket signaling with room support.
///
/// This creates a bidirectional WebRTC connection (sendrecv) to a browser.
/// It uses WebSocket signaling with room isolation - multiple peers can share
/// the same signaling server but only communicate within their room.
///
/// Example with events (recommended):
///   var peer = new BassWebRtcPeer("ws://localhost:8080", "studio-1", bassOutputChannel, decodeStream: true);
///
///   peer.Connected += () =>
///   {
///       Console.WriteLine("Connected!");
///       peer.SetupStreams();
///       peer.EnableStats(1000); // Stats every 1 second
///       fromBrowserChan = peer.InputStreamHandle;
///       BassMix.BASS_Mixer_StreamAddChannel(mixer, fromBrowserChan, BASSFlag.BASS_STREAM_AUTOFREE);
///   };
///
///   peer.StatsUpdated += stats =>
///   {
///       Console.WriteLine($"RTT: {stats.RoundTripTime.TotalMilliseconds:F1}ms, Loss: {stats.PacketLossPercent:F2}%");
///   };
///
///   peer.Disconnected += () =>
///   {
///       Console.WriteLine("Disconnected - browser reloaded or connection lost");
///       // Create new peer to accept next connection
///   };
///
///   peer.Error += (code, msg) => Console.WriteLine($"Error {code}: {msg}");
///
///   peer.Connect(); // Non-blocking, events fire on state changes
/// </summary>
public class BassWebRtcPeer : IDisposable
{
    private IntPtr _handle;
    private bool _disposed;

    // Keep delegates alive to prevent GC collection
    private BassWebRtcNative.OnConnectedCallback? _connectedDelegate;
    private BassWebRtcNative.OnDisconnectedCallback? _disconnectedDelegate;
    private BassWebRtcNative.OnErrorCallback? _errorDelegate;
    private BassWebRtcNative.OnStatsCallback? _statsDelegate;

    // For bitrate calculation
    private ulong _lastBytesSent;
    private ulong _lastBytesReceived;
    private DateTime _lastStatsTime;

    /// <summary>
    /// Event fired when the WebRTC connection is established.
    /// Call SetupStreams() in this handler to initialize audio streams.
    /// </summary>
    public event Action? Connected;

    /// <summary>
    /// Event fired when the WebRTC connection is closed or disconnected.
    /// This indicates the browser reloaded, closed, or the connection was lost.
    /// Create a new BassWebRtcPeer instance to accept a new connection.
    /// </summary>
    public event Action? Disconnected;

    /// <summary>
    /// Event fired when an error occurs.
    /// The error code is non-zero, and the message provides details.
    /// </summary>
    public event Action<uint, string>? Error;

    /// <summary>
    /// Event fired periodically with connection statistics.
    /// Enable by calling EnableStats() after connection is established.
    /// </summary>
    public event Action<WebRtcStats>? StatsUpdated;

    /// <summary>
    /// Gets the room ID this peer is connected to.
    /// </summary>
    public string RoomId { get; }

    /// <summary>
    /// Gets the BASS stream handle for received audio (from browser).
    /// Use this to play the audio or add it to a mixer.
    /// Returns 0 if not connected or no audio received yet.
    /// </summary>
    public int InputStreamHandle => _handle != IntPtr.Zero
        ? BassWebRtcNative.BASS_WEBRTC_PeerGetInputStream(_handle)
        : 0;

    /// <summary>
    /// Gets whether the peer is connected.
    /// </summary>
    public bool IsConnected => _handle != IntPtr.Zero && BassWebRtcNative.BASS_WEBRTC_PeerIsConnected(_handle) != 0;

    /// <summary>
    /// Create a WebRTC peer for bidirectional audio via WebSocket signaling.
    /// </summary>
    /// <param name="signalingUrl">WebSocket signaling server URL (e.g., "ws://localhost:8080")</param>
    /// <param name="roomId">Room identifier for signaling isolation (e.g., "studio-1")</param>
    /// <param name="sourceChannel">BASS channel to send audio from (0 if receive-only)</param>
    /// <param name="sampleRate">Sample rate (48000 recommended for WebRTC)</param>
    /// <param name="channels">Number of channels (1 or 2)</param>
    /// <param name="opusBitrate">OPUS bitrate in kbps for sending (default 128)</param>
    /// <param name="bufferMs">Buffer size in ms for received audio (default 100)</param>
    /// <param name="decodeStream">If true, creates input stream with BASS_STREAM_DECODE flag</param>
    public BassWebRtcPeer(
        string signalingUrl,
        string roomId,
        int sourceChannel,
        uint sampleRate = 48000,
        ushort channels = 2,
        uint opusBitrate = 128,
        uint bufferMs = 100,
        bool decodeStream = false)
    {
        RoomId = roomId;
        byte decode = decodeStream ? (byte)1 : (byte)0;

        _handle = BassWebRtcNative.BASS_WEBRTC_CreatePeer(
            signalingUrl,
            roomId,
            sourceChannel,
            sampleRate,
            channels,
            opusBitrate,
            bufferMs,
            decode);

        if (_handle == IntPtr.Zero)
            throw new InvalidOperationException($"Failed to create WebRTC peer: BASS error {BassWebRtcNative.BASS_ErrorGetCode()}");

        // Setup native callbacks that fire the C# events
        SetupCallbacks();
    }

    /// <summary>
    /// Setup native callbacks to fire C# events on state changes.
    /// </summary>
    private void SetupCallbacks()
    {
        // Create delegates that invoke our events
        // Keep references to prevent GC collection
        _connectedDelegate = user => Connected?.Invoke();

        _disconnectedDelegate = user => Disconnected?.Invoke();

        _errorDelegate = (code, msgPtr, user) =>
        {
            string msg = msgPtr != IntPtr.Zero
                ? Marshal.PtrToStringAnsi(msgPtr) ?? ""
                : "";
            Error?.Invoke(code, msg);
        };

        // Register with native code
        BassWebRtcNative.BASS_WEBRTC_PeerSetCallbacks(
            _handle,
            _connectedDelegate,
            _disconnectedDelegate,
            _errorDelegate,
            IntPtr.Zero);
    }

    /// <summary>
    /// Start connection to the signaling server (non-blocking).
    ///
    /// This starts the connection process in a background thread and returns immediately.
    /// The Connected event will fire when the WebRTC connection is established.
    /// The Disconnected event will fire when the connection is closed.
    /// </summary>
    /// <returns>True if connection started, false on failure</returns>
    public bool Connect()
    {
        if (_handle == IntPtr.Zero) return false;
        return BassWebRtcNative.BASS_WEBRTC_PeerConnect(_handle) != 0;
    }

    /// <summary>
    /// Setup audio streams after connection is established.
    ///
    /// Call this in the Connected event handler.
    /// This sets up:
    /// - Output stream: BASS source channel -> WebRTC (to browser)
    /// - Input stream: WebRTC -> BASS (from browser)
    /// </summary>
    /// <returns>True on success, false on failure</returns>
    public bool SetupStreams()
    {
        if (_handle == IntPtr.Zero) return false;
        return BassWebRtcNative.BASS_WEBRTC_PeerSetupStreams(_handle) != 0;
    }

    /// <summary>
    /// Enable periodic statistics updates.
    ///
    /// Call this after SetupStreams() to receive StatsUpdated events.
    /// Stats include RTT, packet loss, jitter, and byte/packet counts.
    /// </summary>
    /// <param name="intervalMs">Interval in milliseconds between stats updates (default 1000ms)</param>
    /// <returns>True on success, false on failure</returns>
    public bool EnableStats(uint intervalMs = 1000)
    {
        if (_handle == IntPtr.Zero) return false;

        // Initialize bitrate tracking
        _lastBytesSent = 0;
        _lastBytesReceived = 0;
        _lastStatsTime = DateTime.UtcNow;

        // Create delegate that converts FFI struct to managed WebRtcStats
        _statsDelegate = (ref BassWebRtcNative.WebRtcPeerStatsFFI ffi, IntPtr user) =>
        {
            var now = DateTime.UtcNow;
            var elapsed = (now - _lastStatsTime).TotalSeconds;

            var stats = new WebRtcStats
            {
                PacketsSent = ffi.PacketsSent,
                PacketsReceived = ffi.PacketsReceived,
                BytesSent = ffi.BytesSent,
                BytesReceived = ffi.BytesReceived,
                RoundTripTime = TimeSpan.FromMilliseconds(ffi.RoundTripTimeMs),
                PacketsLost = ffi.PacketsLost,
                PacketLossPercent = ffi.FractionLostPercent,
                Jitter = TimeSpan.FromMilliseconds(ffi.JitterMs),
                NackCount = ffi.NackCount,
            };

            // Calculate bitrates
            if (elapsed > 0)
            {
                stats.SendBitrateKbps = (ffi.BytesSent - _lastBytesSent) * 8.0 / 1000.0 / elapsed;
                stats.ReceiveBitrateKbps = (ffi.BytesReceived - _lastBytesReceived) * 8.0 / 1000.0 / elapsed;
            }

            _lastBytesSent = ffi.BytesSent;
            _lastBytesReceived = ffi.BytesReceived;
            _lastStatsTime = now;

            StatsUpdated?.Invoke(stats);
        };

        return BassWebRtcNative.BASS_WEBRTC_PeerSetStatsCallback(
            _handle,
            _statsDelegate,
            intervalMs,
            IntPtr.Zero) != 0;
    }

    /// <summary>
    /// Disconnect from the WebRTC peer.
    /// </summary>
    public void Disconnect()
    {
        if (_handle != IntPtr.Zero)
            BassWebRtcNative.BASS_WEBRTC_PeerDisconnect(_handle);
    }

    /// <summary>
    /// Dispose of the peer and release all resources.
    /// </summary>
    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;

        if (_handle != IntPtr.Zero)
        {
            BassWebRtcNative.BASS_WEBRTC_PeerFree(_handle);
            _handle = IntPtr.Zero;
        }

        // Clear delegate references
        _connectedDelegate = null;
        _disconnectedDelegate = null;
        _errorDelegate = null;
        _statsDelegate = null;

        GC.SuppressFinalize(this);
    }

    ~BassWebRtcPeer() => Dispose();
}
