using System.Runtime.InteropServices;

/// <summary>
/// AES67 constants and P/Invoke declarations for bass_aes67.dll or bass_aes67.so
/// All constants from bass_aes67.h for direct BASS function calls
/// </summary>
public static class Aes67Native
{   

    // Channel type
    public const int BASS_CTYPE_STREAM_AES67 = 0x1f200;

    // General settings
    public const int BASS_CONFIG_AES67_PT = 0x20000;            // RTP payload type (default 96)
    public const int BASS_CONFIG_AES67_INTERFACE = 0x20001;     // Network interface IP (string ptr)
    public const int BASS_CONFIG_AES67_JITTER = 0x20002;        // Jitter buffer depth in ms

    // PTP settings
    public const int BASS_CONFIG_AES67_PTP_DOMAIN = 0x20003;    // PTP domain (default 0)
    public const int BASS_CONFIG_AES67_PTP_STATS = 0x20004;     // PTP stats string (read-only, ptr)
    public const int BASS_CONFIG_AES67_PTP_OFFSET = 0x20005;    // PTP offset in nanoseconds (read-only, i64)
    public const int BASS_CONFIG_AES67_PTP_STATE = 0x20006;     // PTP state (read-only)
    public const int BASS_CONFIG_AES67_PTP_ENABLED = 0x20007;   // Enable/disable PTP (default 1)

    // Stream statistics (read-only)
    public const int BASS_CONFIG_AES67_BUFFER_LEVEL = 0x20010;      // Buffer fill % (0-200, 100=target)
    public const int BASS_CONFIG_AES67_JITTER_UNDERRUNS = 0x20011;  // Jitter buffer underrun count
    public const int BASS_CONFIG_AES67_PACKETS_RECEIVED = 0x20012;  // Total packets received
    public const int BASS_CONFIG_AES67_PACKETS_LATE = 0x20013;      // Late/dropped packet count
    public const int BASS_CONFIG_AES67_BUFFER_PACKETS = 0x20014;    // Current buffer level in packets
    public const int BASS_CONFIG_AES67_TARGET_PACKETS = 0x20015;    // Target buffer level in packets
    public const int BASS_CONFIG_AES67_PACKET_TIME = 0x20016;       // Detected packet time in microseconds

    // Clock status (read-only)
    public const int BASS_CONFIG_AES67_PTP_LOCKED = 0x20017;    // Clock locked (0=no, 1=yes)
    public const int BASS_CONFIG_AES67_PTP_FREQ = 0x20018;      // Frequency PPM x 1000 (signed)

    // Clock settings
    public const int BASS_CONFIG_AES67_CLOCK_MODE = 0x20019;             // Clock mode
    public const int BASS_CONFIG_AES67_CLOCK_FALLBACK_TIMEOUT = 0x2001A; // Fallback timeout secs (0=disabled, default 5)

    // Clock mode values (for BASS_CONFIG_AES67_CLOCK_MODE)
    public const int BASS_AES67_CLOCK_PTP = 0;       // IEEE 1588v2 PTP (default)
    public const int BASS_AES67_CLOCK_LIVEWIRE = 1;  // Axia Livewire Clock
    public const int BASS_AES67_CLOCK_SYSTEM = 2;    // System clock (free-running, no sync)

    // Clock state values (for BASS_CONFIG_AES67_PTP_STATE)
    public const int BASS_AES67_PTP_DISABLED = 0;     // Clock not running
    public const int BASS_AES67_PTP_LISTENING = 1;    // Waiting for master
    public const int BASS_AES67_PTP_UNCALIBRATED = 2; // Syncing with master
    public const int BASS_AES67_PTP_SLAVE = 3;        // Locked to master (or fallback active)

    // P/Invoke for string/pointer config (not in Bass.NET wrapper)
    [DllImport("bass", CharSet = CharSet.Ansi)]
    public static extern bool BASS_SetConfigPtr(int option, string value);

    [DllImport("bass")]
    public static extern IntPtr BASS_GetConfigPtr(int option);

    // Direct P/Invoke for BASS_StreamCreateURL - bypasses Bass.NET
    // Use this to test if Bass.NET wrapper is causing the hang
    [DllImport("bass", EntryPoint = "BASS_StreamCreateURL", CharSet = CharSet.Ansi)]
    public static extern int BASS_StreamCreateURL_Direct(
        [MarshalAs(UnmanagedType.LPStr)] string url,
        int offset,
        int flags,
        IntPtr proc,
        IntPtr user);

    // BASS flag for decode mode
    public const int BASS_STREAM_DECODE = 0x200000;

    // Clock control functions (for output-only mode without input streams)
    // Set BASS_CONFIG_AES67_INTERFACE, BASS_CONFIG_AES67_CLOCK_MODE, and
    // BASS_CONFIG_AES67_PTP_DOMAIN before calling BASS_AES67_ClockStart()



    

    /// <summary>
    /// Start clock independently (for output-only mode without AES67 input streams)
    /// </summary>
    [DllImport("bass_aes67")]
    public static extern bool BASS_AES67_ClockStart();

    /// <summary>
    /// Stop clock
    /// </summary>
    [DllImport("bass_aes67")]
    public static extern bool BASS_AES67_ClockStop();

    /// <summary>
    /// Check if clock is locked (stable synchronization)
    /// </summary>
    [DllImport("bass_aes67")]
    public static extern int BASS_AES67_ClockIsLocked();

    /// <summary>
    /// Get detailed clock stats string (master ID, offset, delay, frequency, state)
    /// Returns pointer to null-terminated string, valid until next call
    /// </summary>
    [DllImport("bass_aes67")]
    public static extern IntPtr BASS_AES67_GetClockStats();

    /// <summary>
    /// Get clock stats as managed string
    /// </summary>
    public static string? GetClockStats()
    {
        IntPtr ptr = BASS_AES67_GetClockStats();
        return ptr == IntPtr.Zero ? null : Marshal.PtrToStringAnsi(ptr);
    }

    /// <summary>
    /// Get string value from pointer config option
    /// </summary>
    public static string? GetConfigString(int option)
    {
        IntPtr ptr = BASS_GetConfigPtr(option);
        return ptr == IntPtr.Zero ? null : Marshal.PtrToStringAnsi(ptr);
    }

    /// <summary>
    /// Get clock state name for display
    /// </summary>
    public static string GetClockStateName(int state) => state switch
    {
        BASS_AES67_PTP_DISABLED => "Disabled",
        BASS_AES67_PTP_LISTENING => "Listening",
        BASS_AES67_PTP_UNCALIBRATED => "Uncalibrated",
        BASS_AES67_PTP_SLAVE => "Locked",
        _ => $"Unknown({state})"
    };

    /// <summary>
    /// Get clock mode name for display
    /// </summary>
    public static string GetClockModeName(int mode) => mode switch
    {
        BASS_AES67_CLOCK_PTP => "PTP",
        BASS_AES67_CLOCK_LIVEWIRE => "Livewire",
        BASS_AES67_CLOCK_SYSTEM => "System",
        _ => $"Unknown({mode})"
    };

    // =========================================================================
    // AES67 OUTPUT STREAM FFI
    // =========================================================================

    /// <summary>
    /// Create an AES67 output stream from a BASS channel
    /// </summary>
    [DllImport("bass_aes67")]
    public static extern IntPtr BASS_AES67_OutputCreate(int bassChannel, ref Aes67OutputConfigFFI config);

    /// <summary>
    /// Start the output stream (begins transmitting)
    /// </summary>
    [DllImport("bass_aes67")]
    public static extern bool BASS_AES67_OutputStart(IntPtr handle);

    /// <summary>
    /// Stop the output stream (stops transmitting, can be restarted)
    /// </summary>
    [DllImport("bass_aes67")]
    public static extern bool BASS_AES67_OutputStop(IntPtr handle);

    /// <summary>
    /// Get output stream statistics (lock-free)
    /// </summary>
    [DllImport("bass_aes67")]
    public static extern bool BASS_AES67_OutputGetStats(IntPtr handle, out OutputStatsFFI stats);

    /// <summary>
    /// Check if output is running
    /// </summary>
    [DllImport("bass_aes67")]
    public static extern bool BASS_AES67_OutputIsRunning(IntPtr handle);

    /// <summary>
    /// Get applied PPM frequency correction (returns PPM x 1000)
    /// </summary>
    [DllImport("bass_aes67")]
    public static extern int BASS_AES67_OutputGetPPM(IntPtr handle);

    /// <summary>
    /// Destroy the output stream and free resources
    /// </summary>
    [DllImport("bass_aes67")]
    public static extern bool BASS_AES67_OutputFree(IntPtr handle);
}

/// <summary>
/// FFI config struct for AES67 output - must match Rust Aes67OutputConfigFFI layout
/// </summary>
[StructLayout(LayoutKind.Sequential)]
public struct Aes67OutputConfigFFI
{
    /// <summary>Multicast IP as 4 bytes (a.b.c.d)</summary>
    [MarshalAs(UnmanagedType.ByValArray, SizeConst = 4)]
    public byte[] MulticastAddr;

    /// <summary>UDP port (typically 5004)</summary>
    public ushort Port;

    /// <summary>Interface IP as 4 bytes (0.0.0.0 for default)</summary>
    [MarshalAs(UnmanagedType.ByValArray, SizeConst = 4)]
    public byte[] InterfaceAddr;

    /// <summary>RTP payload type (typically 96)</summary>
    public byte PayloadType;

    /// <summary>Number of audio channels</summary>
    public ushort Channels;

    /// <summary>Sample rate in Hz (typically 48000)</summary>
    public uint SampleRate;

    /// <summary>Packet time in microseconds (250, 1000, 5000)</summary>
    public uint PacketTimeUs;
}

/// <summary>
/// FFI stats struct for AES67 output - must match Rust OutputStatsFFI layout
/// </summary>
[StructLayout(LayoutKind.Sequential)]
public struct OutputStatsFFI
{
    /// <summary>Total packets transmitted</summary>
    public ulong PacketsSent;

    /// <summary>Total samples transmitted</summary>
    public ulong SamplesSent;

    /// <summary>Transmission errors</summary>
    public ulong SendErrors;

    /// <summary>Buffer underruns</summary>
    public ulong Underruns;
}
