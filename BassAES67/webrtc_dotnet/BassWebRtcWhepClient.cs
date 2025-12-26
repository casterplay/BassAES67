namespace BassWebRtc;

/// <summary>
/// WHEP client for receiving audio FROM an external WHEP server (like MediaMTX).
///
/// Use this to receive audio from a WHEP-compatible server into a BASS stream.
/// The server typically receives audio via WHIP from a browser or other source.
///
/// Example:
///   using var whep = new BassWebRtcWhepClient("http://localhost:8889/mystream/whep");
///   int bassStream = whep.StreamHandle;
///   BASS_ChannelPlay(bassStream, false);
///   // Audio from the WHEP server is now playing
/// </summary>
public class BassWebRtcWhepClient : IDisposable
{
    private IntPtr _handle;
    private bool _disposed;

    /// <summary>
    /// Gets the BASS stream handle for received audio.
    /// Use this to play the audio or add it to a mixer.
    /// </summary>
    public int StreamHandle { get; private set; }

    /// <summary>
    /// Gets whether the client is connected to the WHEP server.
    /// </summary>
    public bool IsConnected => _handle != IntPtr.Zero && BassWebRtcNative.BASS_WEBRTC_WhepIsConnected(_handle) != 0;

    /// <summary>
    /// Create a WHEP client to receive audio from an external server.
    /// </summary>
    /// <param name="whepUrl">WHEP endpoint URL (e.g., "http://localhost:8889/mystream/whep")</param>
    /// <param name="sampleRate">Sample rate (48000 recommended for WebRTC)</param>
    /// <param name="channels">Number of channels (1 or 2)</param>
    /// <param name="bufferMs">Buffer size in milliseconds (default 100)</param>
    /// <param name="decodeStream">If true, creates stream with BASS_STREAM_DECODE flag for mixer compatibility</param>
    public BassWebRtcWhepClient(
        string whepUrl,
        uint sampleRate = 48000,
        ushort channels = 2,
        uint bufferMs = 100,
        bool decodeStream = false)
    {
        byte decode = decodeStream ? (byte)1 : (byte)0;
        _handle = BassWebRtcNative.BASS_WEBRTC_ConnectWhep(whepUrl, sampleRate, channels, bufferMs, decode);
        if (_handle == IntPtr.Zero)
            throw new InvalidOperationException($"Failed to connect WHEP client: BASS error {BassWebRtcNative.BASS_ErrorGetCode()}");

        StreamHandle = BassWebRtcNative.BASS_WEBRTC_WhepGetStream(_handle);
    }

    /// <summary>
    /// Dispose of the WHEP client and release all resources.
    /// </summary>
    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;

        if (_handle != IntPtr.Zero)
        {
            BassWebRtcNative.BASS_WEBRTC_WhepFree(_handle);
            _handle = IntPtr.Zero;
        }

        StreamHandle = 0;
        GC.SuppressFinalize(this);
    }

    ~BassWebRtcWhepClient() => Dispose();
}
