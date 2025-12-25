using System.Runtime.InteropServices;

/// <summary>
/// RTP constants and P/Invoke declarations for bass_rtp plugin.
/// Matches the FFI definitions in bass-rtp/src/lib.rs
/// </summary>
public static class BassRtpNative
{
    // =========================================================================
    // Codec Constants (BASS_RTP_CODEC_*)
    // =========================================================================
    public const byte BASS_RTP_CODEC_PCM16 = 0;   // PCM 16-bit
    public const byte BASS_RTP_CODEC_PCM20 = 1;   // PCM 20-bit (packed)
    public const byte BASS_RTP_CODEC_PCM24 = 2;   // PCM 24-bit
    public const byte BASS_RTP_CODEC_MP2 = 3;     // MPEG-1 Layer 2
    public const byte BASS_RTP_CODEC_G711 = 4;    // G.711 u-Law
    public const byte BASS_RTP_CODEC_G722 = 5;    // G.722

    // =========================================================================
    // Buffer Mode Constants (BASS_RTP_BUFFER_MODE_*)
    // =========================================================================
    public const byte BASS_RTP_BUFFER_MODE_SIMPLE = 0;
    public const byte BASS_RTP_BUFFER_MODE_MINMAX = 1;

    // =========================================================================
    // Clock Mode Constants (BASS_RTP_CLOCK_*)
    // =========================================================================
    public const byte BASS_RTP_CLOCK_PTP = 0;
    public const byte BASS_RTP_CLOCK_LIVEWIRE = 1;
    public const byte BASS_RTP_CLOCK_SYSTEM = 2;

    // =========================================================================
    // Connection State Constants
    // =========================================================================
    public const uint CONNECTION_STATE_DISCONNECTED = 0;
    public const uint CONNECTION_STATE_CONNECTED = 1;

    // =========================================================================
    // RTP Output Configuration Structure
    // Must match RtpOutputConfigFFI in lib.rs exactly
    // =========================================================================
    [StructLayout(LayoutKind.Sequential)]
    public struct RtpOutputConfigFFI
    {
        /// <summary>Local port to listen on (Z/IP ONE connects here)</summary>
        public ushort LocalPort;

        /// <summary>Network interface IP address (4 bytes, 0.0.0.0 = any)</summary>
        [MarshalAs(UnmanagedType.ByValArray, SizeConst = 4)]
        public byte[] InterfaceAddr;

        /// <summary>Sample rate (48000)</summary>
        public uint SampleRate;

        /// <summary>Number of channels (1 or 2)</summary>
        public ushort Channels;

        /// <summary>Backfeed codec (BASS_RTP_CODEC_*)</summary>
        public byte BackfeedCodec;

        /// <summary>Backfeed bitrate in kbps (for MP2, 0 = default 256)</summary>
        public uint BackfeedBitrate;

        /// <summary>Frame duration in milliseconds (1-5, 0 = default 1)</summary>
        public uint FrameDurationMs;

        /// <summary>Clock mode (BASS_RTP_CLOCK_*)</summary>
        public byte ClockMode;

        /// <summary>PTP domain (0-127)</summary>
        public byte PtpDomain;

        /// <summary>Incoming audio buffer mode (BASS_RTP_BUFFER_MODE_*)</summary>
        public byte BufferMode;

        /// <summary>Incoming audio buffer in milliseconds</summary>
        public uint BufferMs;

        /// <summary>Incoming audio max buffer in milliseconds (min/max mode only)</summary>
        public uint MaxBufferMs;

        /// <summary>Create incoming stream with BASS_STREAM_DECODE flag (for mixer compatibility)</summary>
        public byte DecodeStream;

        /// <summary>Connection state callback (optional, can be IntPtr.Zero)</summary>
        public IntPtr ConnectionCallback;

        /// <summary>User data for callback</summary>
        public IntPtr CallbackUserData;

        /// <summary>
        /// Create a default configuration
        /// </summary>
        public static RtpOutputConfigFFI CreateDefault(ushort localPort, string? interfaceIp = null)
        {
            byte[] ifAddr = [0, 0, 0, 0];
            if (!string.IsNullOrEmpty(interfaceIp))
            {
                var parts = interfaceIp.Split('.');
                if (parts.Length == 4)
                {
                    ifAddr = [
                        byte.Parse(parts[0]),
                        byte.Parse(parts[1]),
                        byte.Parse(parts[2]),
                        byte.Parse(parts[3])
                    ];
                }
            }

            return new RtpOutputConfigFFI
            {
                LocalPort = localPort,
                InterfaceAddr = ifAddr,
                SampleRate = 48000,
                Channels = 2,
                BackfeedCodec = BASS_RTP_CODEC_G711,
                BackfeedBitrate = 0,
                FrameDurationMs = 1,
                ClockMode = BASS_RTP_CLOCK_SYSTEM,
                PtpDomain = 0,
                BufferMode = BASS_RTP_BUFFER_MODE_SIMPLE,
                BufferMs = 200,
                MaxBufferMs = 500,
                DecodeStream = 0,
                ConnectionCallback = IntPtr.Zero,
                CallbackUserData = IntPtr.Zero
            };
        }
    }

    // =========================================================================
    // RTP Output Statistics Structure
    // Must match RtpOutputStatsFFI in lib.rs exactly
    // =========================================================================
    [StructLayout(LayoutKind.Sequential)]
    public struct RtpOutputStatsFFI
    {
        /// <summary>RX packets received (incoming audio)</summary>
        public ulong RxPackets;

        /// <summary>RX bytes received</summary>
        public ulong RxBytes;

        /// <summary>RX decode errors</summary>
        public ulong RxDecodeErrors;

        /// <summary>RX packets dropped (buffer full)</summary>
        public ulong RxDropped;

        /// <summary>TX packets sent (backfeed)</summary>
        public ulong TxPackets;

        /// <summary>TX bytes sent</summary>
        public ulong TxBytes;

        /// <summary>TX encode errors</summary>
        public ulong TxEncodeErrors;

        /// <summary>TX buffer underruns</summary>
        public ulong TxUnderruns;

        /// <summary>Current incoming buffer level (samples)</summary>
        public uint BufferLevel;

        /// <summary>Detected incoming audio payload type</summary>
        public byte DetectedIncomingPt;

        /// <summary>Current PPM adjustment (scaled by 1000)</summary>
        public int CurrentPpmX1000;
    }

    // =========================================================================
    // Connection State Callback Delegate
    // =========================================================================
    /// <summary>
    /// Delegate for connection state change callbacks.
    /// Called when Z/IP ONE connects or disconnects.
    /// </summary>
    /// <param name="state">New connection state (CONNECTION_STATE_*)</param>
    /// <param name="user">User data pointer passed to config</param>
    [UnmanagedFunctionPointer(CallingConvention.StdCall)]
    public delegate void ConnectionStateCallback(uint state, IntPtr user);

    // =========================================================================
    // RTP Output Module P/Invoke (Z/IP ONE connects TO us)
    // =========================================================================

    /// <summary>
    /// Create an RTP Output stream (Z/IP ONE connects TO us).
    /// </summary>
    /// <param name="backfeedChannel">BASS channel to read audio FROM to send as backfeed</param>
    /// <param name="config">Stream configuration</param>
    /// <returns>Opaque handle to the RTP Output stream, or IntPtr.Zero on failure</returns>
    [DllImport("bass_rtp", CallingConvention = CallingConvention.StdCall)]
    public static extern IntPtr BASS_RTP_OutputCreate(int backfeedChannel, ref RtpOutputConfigFFI config);

    /// <summary>
    /// Start the RTP Output stream (listening for connections).
    /// </summary>
    /// <param name="handle">Handle from BASS_RTP_OutputCreate</param>
    /// <returns>1 on success, 0 on failure</returns>
    [DllImport("bass_rtp", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_RTP_OutputStart(IntPtr handle);

    /// <summary>
    /// Stop the RTP Output stream.
    /// </summary>
    /// <param name="handle">Handle from BASS_RTP_OutputCreate</param>
    /// <returns>1 on success, 0 on failure</returns>
    [DllImport("bass_rtp", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_RTP_OutputStop(IntPtr handle);

    /// <summary>
    /// Get the incoming audio stream handle (audio received FROM Z/IP ONE).
    /// </summary>
    /// <param name="handle">Handle from BASS_RTP_OutputCreate</param>
    /// <returns>BASS stream handle for incoming audio, or 0 if not available</returns>
    [DllImport("bass_rtp", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_RTP_OutputGetInputStream(IntPtr handle);

    /// <summary>
    /// Get statistics for the RTP Output stream.
    /// </summary>
    /// <param name="handle">Handle from BASS_RTP_OutputCreate</param>
    /// <param name="stats">Structure to fill with statistics</param>
    /// <returns>1 on success, 0 on failure</returns>
    [DllImport("bass_rtp", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_RTP_OutputGetStats(IntPtr handle, out RtpOutputStatsFFI stats);

    /// <summary>
    /// Check if the RTP Output stream is running.
    /// </summary>
    /// <param name="handle">Handle from BASS_RTP_OutputCreate</param>
    /// <returns>1 if running, 0 if not running or invalid handle</returns>
    [DllImport("bass_rtp", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_RTP_OutputIsRunning(IntPtr handle);

    /// <summary>
    /// Free resources associated with an RTP Output stream.
    /// </summary>
    /// <param name="handle">Handle from BASS_RTP_OutputCreate</param>
    /// <returns>1 on success, 0 on failure</returns>
    [DllImport("bass_rtp", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_RTP_OutputFree(IntPtr handle);

    // =========================================================================
    // BASS Core P/Invoke (also available via Bass.NET, but included for convenience)
    // =========================================================================

    [DllImport("bass")]
    public static extern bool BASS_Init(int device, uint freq, uint flags, IntPtr win, IntPtr clsid);

    [DllImport("bass")]
    public static extern bool BASS_Free();

    [DllImport("bass")]
    public static extern int BASS_ErrorGetCode();

    [DllImport("bass")]
    public static extern int BASS_PluginLoad([MarshalAs(UnmanagedType.LPStr)] string file, uint flags);

    [DllImport("bass")]
    public static extern bool BASS_PluginFree(int handle);

    // =========================================================================
    // Helper Methods
    // =========================================================================

    /// <summary>
    /// Get codec name from codec ID
    /// </summary>
    public static string GetCodecName(byte codec) => codec switch
    {
        BASS_RTP_CODEC_PCM16 => "PCM16",
        BASS_RTP_CODEC_PCM20 => "PCM20",
        BASS_RTP_CODEC_PCM24 => "PCM24",
        BASS_RTP_CODEC_MP2 => "MP2",
        BASS_RTP_CODEC_G711 => "G.711u",
        BASS_RTP_CODEC_G722 => "G.722",
        _ => "Unknown"
    };

    /// <summary>
    /// Get clock mode name
    /// </summary>
    public static string GetClockModeName(byte mode) => mode switch
    {
        BASS_RTP_CLOCK_PTP => "PTP",
        BASS_RTP_CLOCK_LIVEWIRE => "Livewire",
        BASS_RTP_CLOCK_SYSTEM => "System",
        _ => "Unknown"
    };

    /// <summary>
    /// Get PPM value from scaled integer (x1000)
    /// </summary>
    public static double GetPpm(int ppmX1000) => ppmX1000 / 1000.0;

    /// <summary>
    /// Get connection state name
    /// </summary>
    public static string GetConnectionStateName(uint state) => state switch
    {
        CONNECTION_STATE_DISCONNECTED => "Disconnected",
        CONNECTION_STATE_CONNECTED => "Connected",
        _ => "Unknown"
    };

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
