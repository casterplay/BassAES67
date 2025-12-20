using System.Net;

/// <summary>
/// AES67 output stream - thin wrapper over Rust implementation.
/// All timing-critical TX code runs in Rust for deterministic performance.
/// </summary>
public class Aes67OutputStream : IDisposable
{
    private IntPtr _handle;
    private readonly Aes67OutputConfig _config;
    private bool _disposed;

    public Aes67OutputConfig Config => _config;
    public bool IsRunning => _handle != IntPtr.Zero && Aes67Native.BASS_AES67_OutputIsRunning(_handle);

    public Aes67OutputStream(Aes67OutputConfig config)
    {
        _config = config;
    }

    /// <summary>
    /// Start transmitting from BASS channel
    /// </summary>
    public void Start(int bassChannel)
    {
        if (_handle != IntPtr.Zero) return;

        // Convert config to FFI struct
        var ffiConfig = new Aes67OutputConfigFFI
        {
            MulticastAddr = _config.MulticastAddr.GetAddressBytes(),
            Port = _config.Port,
            InterfaceAddr = _config.Interface?.GetAddressBytes() ?? [0, 0, 0, 0],
            PayloadType = _config.PayloadType,
            Channels = _config.Channels,
            SampleRate = _config.SampleRate,
            PacketTimeUs = _config.PacketTimeUs
        };

        _handle = Aes67Native.BASS_AES67_OutputCreate(bassChannel, ref ffiConfig);
        if (_handle == IntPtr.Zero)
            throw new InvalidOperationException("Failed to create AES67 output stream");

        if (!Aes67Native.BASS_AES67_OutputStart(_handle))
            throw new InvalidOperationException("Failed to start AES67 output stream");
    }

    /// <summary>
    /// Stop transmitting
    /// </summary>
    public void Stop()
    {
        if (_handle != IntPtr.Zero)
        {
            Aes67Native.BASS_AES67_OutputStop(_handle);
        }
    }

    /// <summary>
    /// Get current statistics from Rust (lock-free)
    /// </summary>
    public (ulong PacketsSent, ulong Underruns, ulong SendErrors) GetStats()
    {
        if (_handle == IntPtr.Zero) return (0, 0, 0);

        if (Aes67Native.BASS_AES67_OutputGetStats(_handle, out var stats))
            return (stats.PacketsSent, stats.Underruns, stats.SendErrors);

        return (0, 0, 0);
    }

    /// <summary>
    /// Get applied PPM frequency correction
    /// </summary>
    public double GetAppliedPPM()
    {
        if (_handle == IntPtr.Zero) return 0.0;
        return Aes67Native.BASS_AES67_OutputGetPPM(_handle) / 1000.0;
    }

    // Backward-compatible properties
    public long PacketsSent => (long)GetStats().PacketsSent;
    public long Underruns => (long)GetStats().Underruns;
    public long SendErrors => (long)GetStats().SendErrors;

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;

        if (_handle != IntPtr.Zero)
        {
            Aes67Native.BASS_AES67_OutputFree(_handle);
            _handle = IntPtr.Zero;
        }

        GC.SuppressFinalize(this);
    }

    ~Aes67OutputStream()
    {
        Dispose();
    }
}
