using System.Runtime.InteropServices;

/// <summary>
/// SRT constants and P/Invoke declarations for bass_srt plugin.
/// All constants from bass_srt for direct BASS function calls.
/// </summary>
public static class BassSrtNative
{
    // =========================================================================
    // Channel Type
    // =========================================================================
    public const int BASS_CTYPE_STREAM_SRT = 0x1f300;

    // =========================================================================
    // Config Options - Stream Statistics
    // =========================================================================
    public const int BASS_CONFIG_SRT_BUFFER_LEVEL = 0x21001;      // Buffer fill % (0-200, 100=target)
    public const int BASS_CONFIG_SRT_PACKETS_RECEIVED = 0x21002;  // Total packets received
    public const int BASS_CONFIG_SRT_PACKETS_DROPPED = 0x21003;   // Dropped packet count
    public const int BASS_CONFIG_SRT_UNDERRUNS = 0x21004;         // Buffer underrun count
    public const int BASS_CONFIG_SRT_CODEC = 0x21005;             // Detected codec type
    public const int BASS_CONFIG_SRT_BITRATE = 0x21006;           // Detected bitrate (kbps)

    // =========================================================================
    // Config Options - SRT Transport Statistics
    // =========================================================================
    public const int BASS_CONFIG_SRT_RTT = 0x21020;               // Round-trip time (ms x 10)
    public const int BASS_CONFIG_SRT_BANDWIDTH = 0x21021;         // Estimated bandwidth (kbps)
    public const int BASS_CONFIG_SRT_SEND_RATE = 0x21022;         // Current send rate (kbps)
    public const int BASS_CONFIG_SRT_RECV_RATE = 0x21023;         // Current receive rate (kbps)
    public const int BASS_CONFIG_SRT_LOSS_TOTAL = 0x21024;        // Total packets lost
    public const int BASS_CONFIG_SRT_RETRANS_TOTAL = 0x21025;     // Total packets retransmitted
    public const int BASS_CONFIG_SRT_DROP_TOTAL = 0x21026;        // Total packets dropped (late)
    public const int BASS_CONFIG_SRT_FLIGHT_SIZE = 0x21027;       // Packets in flight
    public const int BASS_CONFIG_SRT_RECV_BUFFER_MS = 0x21028;    // Receiver buffer level (ms)
    public const int BASS_CONFIG_SRT_UPTIME = 0x21029;            // Connection uptime (seconds)

    // =========================================================================
    // Config Options - Connection State
    // =========================================================================
    public const int BASS_CONFIG_SRT_CONNECTION_STATE = 0x21012;  // Connection state

    // =========================================================================
    // Connection State Values (for BASS_CONFIG_SRT_CONNECTION_STATE)
    // =========================================================================
    public const int CONNECTION_STATE_DISCONNECTED = 0;
    public const int CONNECTION_STATE_CONNECTING = 1;
    public const int CONNECTION_STATE_CONNECTED = 2;
    public const int CONNECTION_STATE_RECONNECTING = 3;

    // =========================================================================
    // Codec Values (for BASS_CONFIG_SRT_CODEC)
    // =========================================================================
    public const int CODEC_UNKNOWN = 0;
    public const int CODEC_PCM = 1;
    public const int CODEC_OPUS = 2;
    public const int CODEC_MP2 = 3;
    public const int CODEC_FLAC = 4;

    // =========================================================================
    // BASS Channel States (for BASS_ChannelIsActive)
    // =========================================================================
    public const int BASS_ACTIVE_STOPPED = 0;
    public const int BASS_ACTIVE_PLAYING = 1;
    public const int BASS_ACTIVE_STALLED = 2;
    public const int BASS_ACTIVE_PAUSED = 3;

    // =========================================================================
    // BASS P/Invoke - Core Functions
    // =========================================================================

    /// <summary>
    /// Initialize BASS audio library
    /// </summary>
    [DllImport("bass")]
    public static extern bool BASS_Init(int device, uint freq, uint flags, IntPtr win, IntPtr clsid);

    /// <summary>
    /// Free BASS resources
    /// </summary>
    [DllImport("bass")]
    public static extern bool BASS_Free();

    /// <summary>
    /// Get BASS version
    /// </summary>
    [DllImport("bass")]
    public static extern uint BASS_GetVersion();

    /// <summary>
    /// Get configuration value
    /// </summary>
    [DllImport("bass")]
    public static extern uint BASS_GetConfig(int option);

    /// <summary>
    /// Set configuration value
    /// </summary>
    [DllImport("bass")]
    public static extern bool BASS_SetConfig(int option, uint value);

    /// <summary>
    /// Get last error code
    /// </summary>
    [DllImport("bass")]
    public static extern int BASS_ErrorGetCode();

    // =========================================================================
    // BASS P/Invoke - Plugin Functions
    // =========================================================================

    /// <summary>
    /// Load a BASS plugin
    /// </summary>
    [DllImport("bass")]
    public static extern int BASS_PluginLoad([MarshalAs(UnmanagedType.LPStr)] string file, uint flags);

    /// <summary>
    /// Free a loaded plugin
    /// </summary>
    [DllImport("bass")]
    public static extern bool BASS_PluginFree(int handle);

    // =========================================================================
    // BASS P/Invoke - Stream Functions
    // =========================================================================

    /// <summary>
    /// Create a stream from a URL (including srt:// URLs with bass_srt plugin)
    /// </summary>
    [DllImport("bass", CharSet = CharSet.Ansi)]
    public static extern int BASS_StreamCreateURL(
        [MarshalAs(UnmanagedType.LPStr)] string url,
        int offset,
        int flags,
        IntPtr proc,
        IntPtr user);

    /// <summary>
    /// Free a stream
    /// </summary>
    [DllImport("bass")]
    public static extern bool BASS_StreamFree(int handle);

    // =========================================================================
    // BASS P/Invoke - Channel Functions
    // =========================================================================

    /// <summary>
    /// Start playback of a channel
    /// </summary>
    [DllImport("bass")]
    public static extern bool BASS_ChannelPlay(int handle, bool restart);

    /// <summary>
    /// Stop playback of a channel
    /// </summary>
    [DllImport("bass")]
    public static extern bool BASS_ChannelStop(int handle);

    /// <summary>
    /// Pause playback of a channel
    /// </summary>
    [DllImport("bass")]
    public static extern bool BASS_ChannelPause(int handle);

    /// <summary>
    /// Get channel state (BASS_ACTIVE_* values)
    /// </summary>
    [DllImport("bass")]
    public static extern int BASS_ChannelIsActive(int handle);


    // =========================================================================
    // BASS SRT Plugin P/Invoke - Connection State Callback
    // =========================================================================

    /// <summary>
    /// Delegate for connection state change callbacks.
    /// Called from the receiver thread when connection state changes.
    /// </summary>
    /// <param name="state">New connection state (CONNECTION_STATE_*)</param>
    /// <param name="user">User data pointer passed to SetConnectionStateCallback</param>
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void ConnectionStateCallback(uint state, IntPtr user);

    /// <summary>
    /// Set callback for connection state changes.
    /// States: 0=disconnected, 1=connecting, 2=connected, 3=reconnecting
    /// </summary>
    [DllImport("bass_srt")]
    public static extern void BASS_SRT_SetConnectionStateCallback(ConnectionStateCallback callback, IntPtr user);

    /// <summary>
    /// Clear the connection state callback
    /// </summary>
    [DllImport("bass_srt")]
    public static extern void BASS_SRT_ClearConnectionStateCallback();

    // =========================================================================
    // Helper Methods
    // =========================================================================

    /// <summary>
    /// Get codec name from codec ID
    /// </summary>
    public static string GetCodecName(int codec) => codec switch
    {
        CODEC_PCM => "PCM",
        CODEC_OPUS => "OPUS",
        CODEC_MP2 => "MP2",
        CODEC_FLAC => "FLAC",
        _ => "Unknown"
    };

    /// <summary>
    /// Get channel state name
    /// </summary>
    public static string GetStateName(int state) => state switch
    {
        BASS_ACTIVE_STOPPED => "Stopped",
        BASS_ACTIVE_PLAYING => "Playing",
        BASS_ACTIVE_STALLED => "Stalled",
        BASS_ACTIVE_PAUSED => "Paused",
        _ => "Unknown"
    };

    /// <summary>
    /// Get connection state name
    /// </summary>
    public static string GetConnectionStateName(int state) => state switch
    {
        CONNECTION_STATE_DISCONNECTED => "Disconnected",
        CONNECTION_STATE_CONNECTING => "Connecting",
        CONNECTION_STATE_CONNECTED => "Connected",
        CONNECTION_STATE_RECONNECTING => "Reconnecting",
        _ => "Unknown"
    };

    /// <summary>
    /// Get RTT in milliseconds (stored as ms x 10)
    /// </summary>
    public static double GetRttMs() => BASS_GetConfig(BASS_CONFIG_SRT_RTT) / 10.0;

    /// <summary>
    /// Format BASS version as string
    /// </summary>
    public static string GetVersionString()
    {
        uint version = BASS_GetVersion();
        return $"{(version >> 24) & 0xFF}.{(version >> 16) & 0xFF}.{(version >> 8) & 0xFF}.{version & 0xFF}";
    }

    /// <summary>
    /// Extract left channel level (0-32768) from BASS_ChannelGetLevel result
    /// </summary>
    public static int GetLeftLevel(uint level) => (int)(level & 0xFFFF);

    /// <summary>
    /// Extract right channel level (0-32768) from BASS_ChannelGetLevel result
    /// </summary>
    public static int GetRightLevel(uint level) => (int)((level >> 16) & 0xFFFF);

    /// <summary>
    /// Get level as percentage (0-100) for left channel
    /// </summary>
    public static double GetLeftLevelPercent(uint level) => GetLeftLevel(level) / 327.68;

    /// <summary>
    /// Get level as percentage (0-100) for right channel
    /// </summary>
    public static double GetRightLevelPercent(uint level) => GetRightLevel(level) / 327.68;

    // =========================================================================
    // SRT OUTPUT STREAM
    // =========================================================================

    // Output codec constants
    public const int OUTPUT_CODEC_PCM = 0;
    public const int OUTPUT_CODEC_OPUS = 1;
    public const int OUTPUT_CODEC_MP2 = 2;
    public const int OUTPUT_CODEC_FLAC = 3;

    // Output connection mode constants
    public const int OUTPUT_MODE_CALLER = 0;
    public const int OUTPUT_MODE_LISTENER = 1;

    /// <summary>
    /// FFI config struct for SRT output.
    /// Must match the Rust SrtOutputConfigFFI layout exactly.
    /// </summary>
    [StructLayout(LayoutKind.Sequential)]
    public struct SrtOutputConfigFFI
    {
        /// <summary>Host IP as 4 bytes (a.b.c.d)</summary>
        [MarshalAs(UnmanagedType.ByValArray, SizeConst = 4)]
        public byte[] HostAddr;

        /// <summary>Port number</summary>
        public ushort Port;

        /// <summary>Connection mode: 0=Caller, 1=Listener</summary>
        public byte Mode;

        /// <summary>SRT latency in milliseconds</summary>
        public uint LatencyMs;

        /// <summary>Passphrase pointer (null-terminated string, or IntPtr.Zero)</summary>
        public IntPtr Passphrase;

        /// <summary>Stream ID pointer (null-terminated string, or IntPtr.Zero)</summary>
        public IntPtr StreamId;

        /// <summary>Number of channels (1 or 2)</summary>
        public ushort Channels;

        /// <summary>Sample rate (e.g., 48000)</summary>
        public uint SampleRate;

        /// <summary>Codec: 0=PCM, 1=OPUS, 2=MP2, 3=FLAC</summary>
        public byte Codec;

        /// <summary>Bitrate in kbps (for OPUS/MP2)</summary>
        public uint BitrateKbps;

        /// <summary>FLAC compression level (0-8)</summary>
        public byte FlacLevel;

        /// <summary>
        /// Create a default configuration for OPUS streaming
        /// </summary>
        public static SrtOutputConfigFFI CreateDefault(string host, ushort port)
        {
            var parts = host.Split('.');
            return new SrtOutputConfigFFI
            {
                HostAddr = new byte[]
                {
                    byte.Parse(parts[0]),
                    byte.Parse(parts[1]),
                    byte.Parse(parts[2]),
                    byte.Parse(parts[3])
                },
                Port = port,
                Mode = OUTPUT_MODE_CALLER,
                LatencyMs = 120,
                Passphrase = IntPtr.Zero,
                StreamId = IntPtr.Zero,
                Channels = 2,
                SampleRate = 48000,
                Codec = OUTPUT_CODEC_OPUS,
                BitrateKbps = 192,
                FlacLevel = 5
            };
        }
    }

    /// <summary>
    /// FFI stats struct for SRT output
    /// </summary>
    [StructLayout(LayoutKind.Sequential)]
    public struct SrtOutputStatsFFI
    {
        public ulong PacketsSent;
        public ulong BytesSent;
        public ulong SendErrors;
        public ulong Underruns;
        public uint ConnectionState;
    }

    /// <summary>
    /// Delegate for output connection state change callbacks.
    /// Called from the transmitter thread when connection state changes.
    /// </summary>
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void OutputConnectionStateCallback(uint state, IntPtr user);

    /// <summary>
    /// Create an SRT output stream from a BASS channel.
    /// </summary>
    /// <param name="bassChannel">BASS channel handle (stream or mixer) to read audio from</param>
    /// <param name="config">Output configuration</param>
    /// <returns>Opaque handle to the output stream, or IntPtr.Zero on error</returns>
    [DllImport("bass_srt")]
    public static extern IntPtr BASS_SRT_OutputCreate(int bassChannel, ref SrtOutputConfigFFI config);

    /// <summary>
    /// Start the SRT output stream (begin transmitting).
    /// </summary>
    /// <returns>true on success, false on failure</returns>
    [DllImport("bass_srt")]
    public static extern bool BASS_SRT_OutputStart(IntPtr handle);

    /// <summary>
    /// Stop the SRT output stream.
    /// </summary>
    /// <returns>true on success, false on failure</returns>
    [DllImport("bass_srt")]
    public static extern bool BASS_SRT_OutputStop(IntPtr handle);

    /// <summary>
    /// Get output stream statistics.
    /// </summary>
    [DllImport("bass_srt")]
    public static extern bool BASS_SRT_OutputGetStats(IntPtr handle, out SrtOutputStatsFFI stats);

    /// <summary>
    /// Check if the output stream is running.
    /// </summary>
    [DllImport("bass_srt")]
    public static extern bool BASS_SRT_OutputIsRunning(IntPtr handle);

    /// <summary>
    /// Destroy the SRT output stream and free resources.
    /// </summary>
    [DllImport("bass_srt")]
    public static extern bool BASS_SRT_OutputFree(IntPtr handle);

    /// <summary>
    /// Set callback for output connection state changes.
    /// States: 0=disconnected, 1=connecting, 2=connected, 3=reconnecting
    /// </summary>
    [DllImport("bass_srt")]
    public static extern void BASS_SRT_SetOutputConnectionStateCallback(
        OutputConnectionStateCallback callback, IntPtr user);

    /// <summary>
    /// Clear the output connection state callback.
    /// </summary>
    [DllImport("bass_srt")]
    public static extern void BASS_SRT_ClearOutputConnectionStateCallback();

    /// <summary>
    /// Get output codec name from codec ID
    /// </summary>
    public static string GetOutputCodecName(int codec) => codec switch
    {
        OUTPUT_CODEC_PCM => "PCM",
        OUTPUT_CODEC_OPUS => "OPUS",
        OUTPUT_CODEC_MP2 => "MP2",
        OUTPUT_CODEC_FLAC => "FLAC",
        _ => "Unknown"
    };
}
