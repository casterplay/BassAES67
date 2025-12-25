using System.Net;
using System.Runtime.InteropServices;

/// <summary>
/// RTP Input stream - WE connect TO Z/IP ONE.
///
/// This is for the scenario where your application initiates the call to Z/IP ONE.
/// You provide a BASS channel as the audio source (what gets sent TO Z/IP ONE),
/// and receive the return audio (what Z/IP ONE sends back to you).
///
/// Z/IP ONE ports:
/// - 9150: Codec negotiation (lowest bitrate)
/// - 9151: Lowest bitrate
/// - 9152: Same codec reply (recommended)
/// - 9153: Highest quality
/// </summary>
public class BassRtpInput : IDisposable
{
    private IntPtr _handle;
    private int _returnStreamHandle;
    private bool _disposed;

    /// <summary>
    /// Handle to the return audio stream (audio received FROM Z/IP ONE).
    /// Use this to play the audio or add to a mixer.
    /// </summary>
    public int ReturnStreamHandle => _returnStreamHandle;

    /// <summary>
    /// Check if the stream is currently running.
    /// </summary>
    public bool IsRunning => _handle != IntPtr.Zero && BassRtpInputNative.BASS_RTP_InputIsRunning(_handle) != 0;

    /// <summary>
    /// Create an RTP Input stream to call Z/IP ONE.
    /// </summary>
    /// <param name="sourceChannel">BASS channel to read audio FROM (sent to Z/IP ONE)</param>
    /// <param name="config">Stream configuration</param>
    public BassRtpInput(int sourceChannel, BassRtpInputNative.RtpInputConfigFFI config)
    {
        _handle = BassRtpInputNative.BASS_RTP_InputCreate(sourceChannel, ref config);
        if (_handle == IntPtr.Zero)
            throw new InvalidOperationException($"Failed to create RTP Input stream: {BassRtpInputNative.BASS_ErrorGetCode()}");
    }

    /// <summary>
    /// Start the RTP stream (begins sending audio to Z/IP ONE).
    /// </summary>
    /// <returns>True on success</returns>
    public bool Start()
    {
        if (_handle == IntPtr.Zero) return false;
        if (BassRtpInputNative.BASS_RTP_InputStart(_handle) == 0) return false;
        _returnStreamHandle = BassRtpInputNative.BASS_RTP_InputGetReturnStream(_handle);
        return true;
    }

    /// <summary>
    /// Stop the RTP stream.
    /// </summary>
    public void Stop()
    {
        if (_handle != IntPtr.Zero)
            BassRtpInputNative.BASS_RTP_InputStop(_handle);
    }

    /// <summary>
    /// Get current statistics.
    /// </summary>
    public BassRtpInputNative.RtpInputStatsFFI GetStats()
    {
        if (_handle != IntPtr.Zero && BassRtpInputNative.BASS_RTP_InputGetStats(_handle, out var stats) != 0)
            return stats;
        return default;
    }

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;
        if (_handle != IntPtr.Zero)
        {
            BassRtpInputNative.BASS_RTP_InputFree(_handle);
            _handle = IntPtr.Zero;
        }
        GC.SuppressFinalize(this);
    }

    ~BassRtpInput() { Dispose(); }
}

/// <summary>
/// P/Invoke declarations for RTP Input module (we connect TO Z/IP ONE).
/// </summary>
public static class BassRtpInputNative
{
    // =========================================================================
    // RTP Input Configuration Structure
    // Must match RtpInputConfigFFI in lib.rs exactly
    // =========================================================================
    [StructLayout(LayoutKind.Sequential)]
    public struct RtpInputConfigFFI
    {
        /// <summary>Remote IP address (Z/IP ONE) as 4 bytes - we connect TO this</summary>
        [MarshalAs(UnmanagedType.ByValArray, SizeConst = 4)]
        public byte[] RemoteAddr;

        /// <summary>Remote port (9150-9153 for Z/IP ONE)</summary>
        public ushort RemotePort;

        /// <summary>Local port to bind (0 = auto-assign)</summary>
        public ushort LocalPort;

        /// <summary>Network interface IP address (4 bytes, 0.0.0.0 = any)</summary>
        [MarshalAs(UnmanagedType.ByValArray, SizeConst = 4)]
        public byte[] InterfaceAddr;

        /// <summary>Sample rate (48000)</summary>
        public uint SampleRate;

        /// <summary>Number of channels (1 or 2)</summary>
        public ushort Channels;

        /// <summary>Send codec (BASS_RTP_CODEC_*)</summary>
        public byte SendCodec;

        /// <summary>Send bitrate in kbps (for MP2, 0 = default 256)</summary>
        public uint SendBitrate;

        /// <summary>Frame duration in milliseconds (1-5, 0 = default 1)</summary>
        public uint FrameDurationMs;

        /// <summary>Clock mode (BASS_RTP_CLOCK_*)</summary>
        public byte ClockMode;

        /// <summary>PTP domain (0-127)</summary>
        public byte PtpDomain;

        /// <summary>Return audio buffer mode (BASS_RTP_BUFFER_MODE_*)</summary>
        public byte ReturnBufferMode;

        /// <summary>Return audio buffer in milliseconds</summary>
        public uint ReturnBufferMs;

        /// <summary>Return audio max buffer in milliseconds (min/max mode only)</summary>
        public uint ReturnMaxBufferMs;

        /// <summary>Create return stream with BASS_STREAM_DECODE flag (for mixer compatibility)</summary>
        public byte DecodeStream;

        /// <summary>
        /// Create a configuration to call Z/IP ONE.
        /// </summary>
        /// <param name="zipOneIp">Z/IP ONE IP address</param>
        /// <param name="port">Z/IP ONE port (9150-9153, default 9152 for same codec reply)</param>
        /// <param name="interfaceIp">Optional: local network interface IP</param>
        public static RtpInputConfigFFI CreateDefault(string zipOneIp, ushort port = 9152, string? interfaceIp = null)
        {
            byte[] remoteAddr = ParseIp(zipOneIp);
            byte[] ifAddr = string.IsNullOrEmpty(interfaceIp) ? [0, 0, 0, 0] : ParseIp(interfaceIp);

            return new RtpInputConfigFFI
            {
                RemoteAddr = remoteAddr,
                RemotePort = port,
                LocalPort = 0,  // Auto-assign
                InterfaceAddr = ifAddr,
                SampleRate = 48000,
                Channels = 2,
                SendCodec = BassRtpNative.BASS_RTP_CODEC_G711,  // G.711 is widely compatible
                SendBitrate = 0,
                FrameDurationMs = 1,
                ClockMode = BassRtpNative.BASS_RTP_CLOCK_SYSTEM,
                PtpDomain = 0,
                ReturnBufferMode = BassRtpNative.BASS_RTP_BUFFER_MODE_SIMPLE,
                ReturnBufferMs = 200,
                ReturnMaxBufferMs = 500,
                DecodeStream = 0
            };
        }

        private static byte[] ParseIp(string ip)
        {
            var parts = ip.Split('.');
            if (parts.Length != 4)
                throw new ArgumentException($"Invalid IP address: {ip}");
            return [
                byte.Parse(parts[0]),
                byte.Parse(parts[1]),
                byte.Parse(parts[2]),
                byte.Parse(parts[3])
            ];
        }
    }

    // =========================================================================
    // RTP Input Statistics Structure
    // Must match RtpInputStatsFFI in lib.rs exactly
    // =========================================================================
    [StructLayout(LayoutKind.Sequential)]
    public struct RtpInputStatsFFI
    {
        /// <summary>TX packets sent (to Z/IP ONE)</summary>
        public ulong TxPackets;

        /// <summary>TX bytes sent</summary>
        public ulong TxBytes;

        /// <summary>TX encode errors</summary>
        public ulong TxEncodeErrors;

        /// <summary>TX buffer underruns</summary>
        public ulong TxUnderruns;

        /// <summary>RX packets received (return audio from Z/IP ONE)</summary>
        public ulong RxPackets;

        /// <summary>RX bytes received</summary>
        public ulong RxBytes;

        /// <summary>RX decode errors</summary>
        public ulong RxDecodeErrors;

        /// <summary>RX packets dropped (buffer full)</summary>
        public ulong RxDropped;

        /// <summary>Current return buffer level (samples)</summary>
        public uint BufferLevel;

        /// <summary>Detected return audio payload type</summary>
        public byte DetectedReturnPt;

        /// <summary>Current PPM adjustment (scaled by 1000)</summary>
        public int CurrentPpmX1000;
    }

    // =========================================================================
    // RTP Input Module P/Invoke (WE connect TO Z/IP ONE)
    // =========================================================================

    /// <summary>
    /// Create an RTP Input stream (WE connect TO Z/IP ONE).
    /// </summary>
    /// <param name="sourceChannel">BASS channel to read audio FROM to send to Z/IP ONE</param>
    /// <param name="config">Stream configuration</param>
    /// <returns>Opaque handle to the RTP Input stream, or IntPtr.Zero on failure</returns>
    [DllImport("bass_rtp", CallingConvention = CallingConvention.StdCall)]
    public static extern IntPtr BASS_RTP_InputCreate(int sourceChannel, ref RtpInputConfigFFI config);

    /// <summary>
    /// Start the RTP Input stream (begins sending to Z/IP ONE).
    /// </summary>
    /// <param name="handle">Handle from BASS_RTP_InputCreate</param>
    /// <returns>1 on success, 0 on failure</returns>
    [DllImport("bass_rtp", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_RTP_InputStart(IntPtr handle);

    /// <summary>
    /// Stop the RTP Input stream.
    /// </summary>
    /// <param name="handle">Handle from BASS_RTP_InputCreate</param>
    /// <returns>1 on success, 0 on failure</returns>
    [DllImport("bass_rtp", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_RTP_InputStop(IntPtr handle);

    /// <summary>
    /// Get the return audio stream handle (audio received FROM Z/IP ONE).
    /// </summary>
    /// <param name="handle">Handle from BASS_RTP_InputCreate</param>
    /// <returns>BASS stream handle for return audio, or 0 if not available</returns>
    [DllImport("bass_rtp", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_RTP_InputGetReturnStream(IntPtr handle);

    /// <summary>
    /// Get statistics for the RTP Input stream.
    /// </summary>
    /// <param name="handle">Handle from BASS_RTP_InputCreate</param>
    /// <param name="stats">Structure to fill with statistics</param>
    /// <returns>1 on success, 0 on failure</returns>
    [DllImport("bass_rtp", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_RTP_InputGetStats(IntPtr handle, out RtpInputStatsFFI stats);

    /// <summary>
    /// Check if the RTP Input stream is running.
    /// </summary>
    /// <param name="handle">Handle from BASS_RTP_InputCreate</param>
    /// <returns>1 if running, 0 if not running or invalid handle</returns>
    [DllImport("bass_rtp", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_RTP_InputIsRunning(IntPtr handle);

    /// <summary>
    /// Free resources associated with an RTP Input stream.
    /// </summary>
    /// <param name="handle">Handle from BASS_RTP_InputCreate</param>
    /// <returns>1 on success, 0 on failure</returns>
    [DllImport("bass_rtp", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_RTP_InputFree(IntPtr handle);

    // BASS error codes
    [DllImport("bass")]
    public static extern int BASS_ErrorGetCode();

    // =========================================================================
    // Helper Methods
    // =========================================================================

    /// <summary>
    /// Get PPM value from scaled integer (x1000)
    /// </summary>
    public static double GetPpm(int ppmX1000) => ppmX1000 / 1000.0;

    /// <summary>
    /// Get detected codec name from RTP payload type
    /// </summary>
    public static string GetPayloadTypeName(byte pt) => pt switch
    {
        0 => "G.711u",
        9 => "G.722",
        10 => "PCM16",
        11 => "PCM16",
        14 => "MP2",
        96 => "PCM16/24",
        _ => $"PT:{pt}"
    };
}
