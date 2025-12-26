namespace BassWebRtc;

/// <summary>
/// WHIP client for pushing audio TO an external WHIP server (like MediaMTX).
///
/// Use this to stream audio from your application to a WHIP-compatible server,
/// which can then be received by browsers via WHEP.
///
/// Example:
///   using var whip = new BassWebRtcWhipClient(bassChannel, "http://localhost:8889/mystream/whip");
///   whip.Start();
///   // Audio is now streaming to the WHIP server
///   // Browsers can connect via WHEP at http://localhost:8889/mystream/whep
/// </summary>
public class BassWebRtcWhipClient : IDisposable
{
    private IntPtr _handle;
    private bool _disposed;

    /// <summary>
    /// Gets whether the client is connected to the WHIP server.
    /// </summary>
    public bool IsConnected => _handle != IntPtr.Zero && BassWebRtcNative.BASS_WEBRTC_WhipIsConnected(_handle) != 0;

    /// <summary>
    /// Create a WHIP client to push audio to an external server.
    /// </summary>
    /// <param name="sourceChannel">BASS channel to read audio from</param>
    /// <param name="whipUrl">WHIP endpoint URL (e.g., "http://localhost:8889/mystream/whip")</param>
    /// <param name="sampleRate">Sample rate (48000 recommended for WebRTC)</param>
    /// <param name="channels">Number of channels (1 or 2)</param>
    /// <param name="opusBitrate">OPUS bitrate in kbps (default 128)</param>
    public BassWebRtcWhipClient(
        int sourceChannel,
        string whipUrl,
        uint sampleRate = 48000,
        ushort channels = 2,
        uint opusBitrate = 128)
    {
        _handle = BassWebRtcNative.BASS_WEBRTC_ConnectWhip(sourceChannel, whipUrl, sampleRate, channels, opusBitrate);
        if (_handle == IntPtr.Zero)
            throw new InvalidOperationException($"Failed to connect WHIP client: BASS error {BassWebRtcNative.BASS_ErrorGetCode()}");
    }

    /// <summary>
    /// Start streaming audio to the WHIP server.
    /// </summary>
    /// <returns>True on success, false on failure</returns>
    public bool Start()
    {
        if (_handle == IntPtr.Zero) return false;
        return BassWebRtcNative.BASS_WEBRTC_WhipStart(_handle) != 0;
    }

    /// <summary>
    /// Stop streaming and disconnect from the WHIP server.
    /// </summary>
    public void Stop()
    {
        if (_handle != IntPtr.Zero)
            BassWebRtcNative.BASS_WEBRTC_WhipStop(_handle);
    }

    /// <summary>
    /// Dispose of the WHIP client and release all resources.
    /// </summary>
    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;

        if (_handle != IntPtr.Zero)
        {
            BassWebRtcNative.BASS_WEBRTC_WhipFree(_handle);
            _handle = IntPtr.Zero;
        }

        GC.SuppressFinalize(this);
    }

    ~BassWebRtcWhipClient() => Dispose();
}
