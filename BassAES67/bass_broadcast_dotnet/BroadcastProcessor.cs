using System.Runtime.InteropServices;

namespace BassProcessor;

/// <summary>
/// Real-time processor statistics.
/// Delivered via callback from the native library.
/// </summary>
public class ProcessorStats
{
    /// <summary>Momentary loudness (LUFS, 400ms window).</summary>
    public float LufsMomentary { get; init; }

    /// <summary>Short-term loudness (LUFS, 3s window).</summary>
    public float LufsShortTerm { get; init; }

    /// <summary>Integrated loudness (LUFS, gated).</summary>
    public float LufsIntegrated { get; init; }

    /// <summary>Input peak level (linear, 0.0 to 1.0+).</summary>
    public float InputPeak { get; init; }

    /// <summary>Output peak level (linear, 0.0 to 1.0+).</summary>
    public float OutputPeak { get; init; }

    /// <summary>Input peak level in dBFS.</summary>
    public float InputPeakDbfs => InputPeak > 0 ? 20f * MathF.Log10(InputPeak) : -100f;

    /// <summary>Output peak level in dBFS.</summary>
    public float OutputPeakDbfs => OutputPeak > 0 ? 20f * MathF.Log10(OutputPeak) : -100f;

    /// <summary>AGC gain reduction in dB (negative when compressing).</summary>
    public float AgcGrDb { get; init; }

    /// <summary>Per-band gain reduction in dB (negative when compressing).</summary>
    public float[] BandGrDb { get; init; } = Array.Empty<float>();

    /// <summary>Number of bands in use.</summary>
    public uint NumBands { get; init; }

    /// <summary>Clipper activity (0.0 = no clipping, 1.0 = constant clipping).</summary>
    public float ClipperActivity { get; init; }

    /// <summary>Total samples (frames) processed.</summary>
    public ulong SamplesProcessed { get; init; }

    /// <summary>Number of source underruns.</summary>
    public ulong Underruns { get; init; }

    /// <summary>Last processing time in microseconds.</summary>
    public ulong ProcessTimeUs { get; init; }

    /// <summary>Processing time as a TimeSpan.</summary>
    public TimeSpan ProcessTime => TimeSpan.FromMicroseconds(ProcessTimeUs);
}

/// <summary>
/// Multiband broadcast audio processor with callback-based statistics.
///
/// This provides real-time audio processing with:
/// - N-band multiband compression (2, 5, or 8 bands)
/// - AGC (single-stage or 3-stage cascaded)
/// - Stereo enhancement
/// - Parametric EQ
/// - Soft clipping with oversampling
/// - LUFS loudness metering
///
/// Example usage:
/// <code>
/// // Create 5-band processor
/// var processor = BroadcastProcessor.Create5Band(sourceChannel);
///
/// // Subscribe to stats events
/// processor.StatsUpdated += stats =>
/// {
///     Console.WriteLine($"LUFS: {stats.LufsMomentary:F1}, Peak: {stats.OutputPeakDbfs:F1} dBFS");
/// };
///
/// // Enable stats callback (100ms interval)
/// processor.EnableStats(100);
///
/// // Get output handle for playback
/// var outputHandle = processor.OutputHandle;
/// Bass.BASS_ChannelPlay(outputHandle, false);
///
/// // Configure processing
/// processor.SetAgc(AgcConfig.Default);
/// processor.IsSoftClipperEnabled = true;
///
/// // When done
/// processor.Dispose();
/// </code>
/// </summary>
public class BroadcastProcessor : IDisposable
{
    private IntPtr _handle;
    private bool _disposed;
    private readonly uint _numBands;

    // Keep delegate alive to prevent GC collection
    private BassProcessorNative.OnStatsCallback? _statsDelegate;

    /// <summary>
    /// Event fired periodically with processor statistics.
    /// Enable by calling EnableStats() with desired interval.
    /// Contains LUFS, peaks, gain reduction, and processing metrics.
    /// </summary>
    public event Action<ProcessorStats>? StatsUpdated;

    /// <summary>
    /// Gets the BASS output stream handle.
    /// Use this for playback or adding to a mixer.
    /// </summary>
    public int OutputHandle => _handle != IntPtr.Zero
        ? BassProcessorNative.BASS_MultibandProcessor_GetOutput(_handle)
        : 0;

    /// <summary>
    /// Gets the number of frequency bands.
    /// </summary>
    public uint NumBands => _numBands;

    /// <summary>
    /// Create a multiband broadcast processor.
    /// </summary>
    /// <param name="sourceChannel">BASS channel handle to pull audio from</param>
    /// <param name="header">Configuration header (sample rate, channels, num bands)</param>
    /// <param name="crossoverFreqs">Crossover frequencies (num_bands - 1 elements)</param>
    /// <param name="bandConfigs">Per-band compressor settings (num_bands elements)</param>
    public BroadcastProcessor(
        int sourceChannel,
        BassProcessorNative.MultibandConfigHeader header,
        float[] crossoverFreqs,
        BassProcessorNative.CompressorConfig[] bandConfigs)
    {
        if (crossoverFreqs.Length != header.NumBands - 1)
            throw new ArgumentException($"Expected {header.NumBands - 1} crossover frequencies, got {crossoverFreqs.Length}");

        if (bandConfigs.Length != header.NumBands)
            throw new ArgumentException($"Expected {header.NumBands} band configs, got {bandConfigs.Length}");

        _numBands = header.NumBands;

        _handle = BassProcessorNative.BASS_MultibandProcessor_Create(
            sourceChannel,
            ref header,
            crossoverFreqs,
            bandConfigs);

        if (_handle == IntPtr.Zero)
            throw new InvalidOperationException("Failed to create broadcast processor");
    }

    /// <summary>
    /// Private constructor for factory methods.
    /// </summary>
    private BroadcastProcessor(IntPtr handle, uint numBands)
    {
        _handle = handle;
        _numBands = numBands;
    }

    /// <summary>
    /// Create a 2-band processor (Low/High split at 2kHz).
    /// Good for basic broadcast processing with less latency.
    /// </summary>
    /// <param name="sourceChannel">BASS channel handle to pull audio from</param>
    /// <param name="sampleRate">Sample rate (default 48000)</param>
    /// <param name="decodeOutput">If true, output is decode-only (for feeding to AES67)</param>
    /// <returns>Configured 2-band processor</returns>
    public static BroadcastProcessor Create2Band(
        int sourceChannel,
        uint sampleRate = 48000,
        bool decodeOutput = false)
    {
        var header = new BassProcessorNative.MultibandConfigHeader
        {
            SampleRate = sampleRate,
            Channels = 2,
            NumBands = 2,
            DecodeOutput = decodeOutput ? (byte)1 : (byte)0,
            InputGainDb = 0.0f,
            OutputGainDb = 0.0f
        };

        float[] crossovers = [2000.0f]; // 2kHz split

        var bands = new BassProcessorNative.CompressorConfig[]
        {
            // Low band: slower attack/release for bass
            new()
            {
                ThresholdDb = -18.0f,
                Ratio = 3.0f,
                AttackMs = 20.0f,
                ReleaseMs = 200.0f,
                MakeupGainDb = 0.0f,
                LookaheadMs = 0.0f
            },
            // High band: faster attack for transients
            new()
            {
                ThresholdDb = -18.0f,
                Ratio = 4.0f,
                AttackMs = 5.0f,
                ReleaseMs = 100.0f,
                MakeupGainDb = 0.0f,
                LookaheadMs = 0.0f
            }
        };

        var handle = BassProcessorNative.BASS_MultibandProcessor_Create(
            sourceChannel,
            ref header,
            crossovers,
            bands);

        if (handle == IntPtr.Zero)
            throw new InvalidOperationException("Failed to create 2-band processor");

        return new BroadcastProcessor(handle, 2);
    }

    /// <summary>
    /// Create a 5-band processor (typical broadcast configuration).
    /// Crossovers: 120Hz, 400Hz, 2kHz, 8kHz
    /// </summary>
    /// <param name="sourceChannel">BASS channel handle to pull audio from</param>
    /// <param name="sampleRate">Sample rate (default 48000)</param>
    /// <param name="decodeOutput">If true, output is decode-only (for feeding to AES67)</param>
    /// <returns>Configured 5-band processor</returns>
    public static BroadcastProcessor Create5Band(
        int sourceChannel,
        uint sampleRate = 48000,
        bool decodeOutput = false)
    {
        var header = new BassProcessorNative.MultibandConfigHeader
        {
            SampleRate = sampleRate,
            Channels = 2,
            NumBands = 5,
            DecodeOutput = decodeOutput ? (byte)1 : (byte)0,
            InputGainDb = 0.0f,
            OutputGainDb = 0.0f
        };

        float[] crossovers = [120.0f, 400.0f, 2000.0f, 8000.0f];

        var bands = new BassProcessorNative.CompressorConfig[]
        {
            // Sub-bass (< 120Hz): very slow, gentle
            new()
            {
                ThresholdDb = -16.0f,
                Ratio = 2.5f,
                AttackMs = 30.0f,
                ReleaseMs = 300.0f,
                MakeupGainDb = 0.0f,
                LookaheadMs = 0.0f
            },
            // Bass (120-400Hz): slow for punch
            new()
            {
                ThresholdDb = -18.0f,
                Ratio = 3.0f,
                AttackMs = 20.0f,
                ReleaseMs = 200.0f,
                MakeupGainDb = 0.0f,
                LookaheadMs = 0.0f
            },
            // Midrange (400Hz-2kHz): medium for vocals
            new()
            {
                ThresholdDb = -20.0f,
                Ratio = 3.5f,
                AttackMs = 10.0f,
                ReleaseMs = 150.0f,
                MakeupGainDb = 0.0f,
                LookaheadMs = 0.0f
            },
            // Presence (2-8kHz): faster for detail
            new()
            {
                ThresholdDb = -20.0f,
                Ratio = 4.0f,
                AttackMs = 5.0f,
                ReleaseMs = 100.0f,
                MakeupGainDb = 0.0f,
                LookaheadMs = 0.0f
            },
            // Brilliance (> 8kHz): fastest for air
            new()
            {
                ThresholdDb = -18.0f,
                Ratio = 3.5f,
                AttackMs = 3.0f,
                ReleaseMs = 80.0f,
                MakeupGainDb = 0.0f,
                LookaheadMs = 0.0f
            }
        };

        var handle = BassProcessorNative.BASS_MultibandProcessor_Create(
            sourceChannel,
            ref header,
            crossovers,
            bands);

        if (handle == IntPtr.Zero)
            throw new InvalidOperationException("Failed to create 5-band processor");

        return new BroadcastProcessor(handle, 5);
    }

    // =========================================================================
    // Stats Callback
    // =========================================================================

    /// <summary>
    /// Enable periodic statistics updates via callback.
    /// Subscribe to the StatsUpdated event to receive notifications.
    /// </summary>
    /// <param name="intervalMs">Interval between updates in milliseconds (50-1000, default 100)</param>
    /// <returns>True on success</returns>
    public bool EnableStats(uint intervalMs = 100)
    {
        if (_handle == IntPtr.Zero) return false;

        // Create delegate that converts FFI struct to managed ProcessorStats
        _statsDelegate = (ref BassProcessorNative.ProcessorStatsCallbackData ffi, IntPtr user) =>
        {
            // Extract per-band GR values
            var bandGr = new float[ffi.NumBands];
            for (int i = 0; i < ffi.NumBands && i < BassProcessorNative.MAX_BANDS; i++)
            {
                bandGr[i] = ffi.BandGrDb[i];
            }

            var stats = new ProcessorStats
            {
                LufsMomentary = ffi.LufsMomentary,
                LufsShortTerm = ffi.LufsShortTerm,
                LufsIntegrated = ffi.LufsIntegrated,
                InputPeak = ffi.InputPeak,
                OutputPeak = ffi.OutputPeak,
                AgcGrDb = ffi.AgcGrDb,
                BandGrDb = bandGr,
                NumBands = ffi.NumBands,
                ClipperActivity = ffi.ClipperActivity,
                SamplesProcessed = ffi.SamplesProcessed,
                Underruns = ffi.Underruns,
                ProcessTimeUs = ffi.ProcessTimeUs
            };

            StatsUpdated?.Invoke(stats);
        };

        return BassProcessorNative.BASS_MultibandProcessor_SetStatsCallback(
            _handle,
            _statsDelegate,
            intervalMs,
            IntPtr.Zero) != 0;
    }

    /// <summary>
    /// Disable statistics updates.
    /// </summary>
    /// <returns>True on success</returns>
    public bool DisableStats()
    {
        if (_handle == IntPtr.Zero) return false;

        var result = BassProcessorNative.BASS_MultibandProcessor_SetStatsCallback(
            _handle,
            null,
            0,
            IntPtr.Zero) != 0;

        _statsDelegate = null;
        return result;
    }

    // =========================================================================
    // Core Controls
    // =========================================================================

    /// <summary>
    /// Set bypass mode (pass audio through without processing).
    /// </summary>
    public bool SetBypass(bool bypass)
    {
        if (_handle == IntPtr.Zero) return false;
        return BassProcessorNative.BASS_MultibandProcessor_SetBypass(_handle, bypass ? 1 : 0) != 0;
    }

    /// <summary>
    /// Set input and output gain.
    /// </summary>
    /// <param name="inputGainDb">Input gain in dB (-20 to +20)</param>
    /// <param name="outputGainDb">Output gain in dB (-20 to +20)</param>
    public bool SetGains(float inputGainDb, float outputGainDb)
    {
        if (_handle == IntPtr.Zero) return false;
        return BassProcessorNative.BASS_MultibandProcessor_SetGains(_handle, inputGainDb, outputGainDb) != 0;
    }

    /// <summary>
    /// Reset all processor state (compressors, AGC, etc.).
    /// </summary>
    public bool Reset()
    {
        if (_handle == IntPtr.Zero) return false;
        return BassProcessorNative.BASS_MultibandProcessor_Reset(_handle) != 0;
    }

    /// <summary>
    /// Pre-fill the processor buffer to reduce initial latency.
    /// </summary>
    public bool Prefill()
    {
        if (_handle == IntPtr.Zero) return false;
        return BassProcessorNative.BASS_MultibandProcessor_Prefill(_handle) != 0;
    }

    /// <summary>
    /// Update a specific band's compressor settings.
    /// </summary>
    /// <param name="band">Band index (0 to NumBands-1)</param>
    /// <param name="config">New compressor configuration</param>
    public bool SetBand(uint band, BassProcessorNative.CompressorConfig config)
    {
        if (_handle == IntPtr.Zero) return false;
        return BassProcessorNative.BASS_MultibandProcessor_SetBand(_handle, band, ref config) != 0;
    }

    // =========================================================================
    // Lookahead
    // =========================================================================

    /// <summary>
    /// Enable or disable lookahead for all compressor bands.
    /// </summary>
    /// <param name="enabled">Enable lookahead</param>
    /// <param name="lookaheadMs">Lookahead time in milliseconds (0.0 to 10.0)</param>
    public bool SetLookahead(bool enabled, float lookaheadMs = 5.0f)
    {
        if (_handle == IntPtr.Zero) return false;
        return BassProcessorNative.BASS_MultibandProcessor_SetLookahead(_handle, enabled ? 1 : 0, lookaheadMs) != 0;
    }

    /// <summary>
    /// Get current lookahead latency in milliseconds.
    /// </summary>
    public float LookaheadLatencyMs => _handle != IntPtr.Zero
        ? BassProcessorNative.BASS_MultibandProcessor_GetLookahead(_handle)
        : 0f;

    // =========================================================================
    // AGC (Automatic Gain Control)
    // =========================================================================

    /// <summary>
    /// Set single-stage AGC configuration.
    /// </summary>
    public bool SetAgc(BassProcessorNative.AgcConfig config)
    {
        if (_handle == IntPtr.Zero) return false;
        return BassProcessorNative.BASS_MultibandProcessor_SetAGC(_handle, ref config) != 0;
    }

    /// <summary>
    /// Set 3-stage cascaded AGC configuration (Omnia 9 style).
    /// </summary>
    public bool SetAgc3Stage(BassProcessorNative.Agc3StageConfig config)
    {
        if (_handle == IntPtr.Zero) return false;
        return BassProcessorNative.BASS_MultibandProcessor_SetAGC3Stage(_handle, ref config) != 0;
    }

    /// <summary>
    /// Check if 3-stage AGC mode is active.
    /// </summary>
    public bool IsAgc3Stage => _handle != IntPtr.Zero &&
        BassProcessorNative.BASS_MultibandProcessor_IsAGC3Stage(_handle) != 0;

    /// <summary>
    /// Get individual stage gain reduction values for 3-stage AGC.
    /// </summary>
    /// <param name="slowGr">Slow stage GR in dB</param>
    /// <param name="mediumGr">Medium stage GR in dB</param>
    /// <param name="fastGr">Fast stage GR in dB</param>
    /// <returns>True if 3-stage mode is active</returns>
    public bool GetAgc3StageGR(out float slowGr, out float mediumGr, out float fastGr)
    {
        if (_handle == IntPtr.Zero)
        {
            slowGr = mediumGr = fastGr = 0f;
            return false;
        }
        return BassProcessorNative.BASS_MultibandProcessor_GetAGC3StageGR(_handle, out slowGr, out mediumGr, out fastGr) != 0;
    }

    // =========================================================================
    // Stereo Enhancer
    // =========================================================================

    /// <summary>
    /// Set stereo enhancer configuration.
    /// </summary>
    public bool SetStereoEnhancer(BassProcessorNative.StereoEnhancerConfig config)
    {
        if (_handle == IntPtr.Zero) return false;
        return BassProcessorNative.BASS_MultibandProcessor_SetStereoEnhancer(_handle, ref config) != 0;
    }

    /// <summary>
    /// Gets or sets whether the stereo enhancer is enabled.
    /// </summary>
    public bool IsStereoEnhancerEnabled
    {
        get => _handle != IntPtr.Zero &&
            BassProcessorNative.BASS_MultibandProcessor_IsStereoEnhancerEnabled(_handle) != 0;
        set
        {
            if (_handle != IntPtr.Zero)
                BassProcessorNative.BASS_MultibandProcessor_SetStereoEnhancerEnabled(_handle, value ? 1 : 0);
        }
    }

    // =========================================================================
    // Parametric EQ
    // =========================================================================

    /// <summary>
    /// Set parametric EQ configuration.
    /// </summary>
    public bool SetParametricEq(BassProcessorNative.ParametricEqConfig config)
    {
        if (_handle == IntPtr.Zero) return false;
        return BassProcessorNative.BASS_MultibandProcessor_SetParametricEQ(_handle, ref config) != 0;
    }

    /// <summary>
    /// Gets or sets whether the parametric EQ is enabled.
    /// </summary>
    public bool IsParametricEqEnabled
    {
        get => _handle != IntPtr.Zero &&
            BassProcessorNative.BASS_MultibandProcessor_IsParametricEQEnabled(_handle) != 0;
        set
        {
            if (_handle != IntPtr.Zero)
                BassProcessorNative.BASS_MultibandProcessor_SetParametricEQEnabled(_handle, value ? 1 : 0);
        }
    }

    // =========================================================================
    // Soft Clipper
    // =========================================================================

    /// <summary>
    /// Set soft clipper configuration.
    /// </summary>
    public bool SetSoftClipper(BassProcessorNative.SoftClipperConfig config)
    {
        if (_handle == IntPtr.Zero) return false;
        return BassProcessorNative.BASS_MultibandProcessor_SetSoftClipper(_handle, ref config) != 0;
    }

    /// <summary>
    /// Gets or sets whether the soft clipper is enabled.
    /// </summary>
    public bool IsSoftClipperEnabled
    {
        get => _handle != IntPtr.Zero &&
            BassProcessorNative.BASS_MultibandProcessor_IsSoftClipperEnabled(_handle) != 0;
        set
        {
            if (_handle != IntPtr.Zero)
                BassProcessorNative.BASS_MultibandProcessor_SetSoftClipperEnabled(_handle, value ? 1 : 0);
        }
    }

    /// <summary>
    /// Get soft clipper latency in milliseconds.
    /// </summary>
    public float SoftClipperLatencyMs => _handle != IntPtr.Zero
        ? BassProcessorNative.BASS_MultibandProcessor_GetSoftClipperLatency(_handle)
        : 0f;

    // =========================================================================
    // LUFS Metering
    // =========================================================================

    /// <summary>
    /// Get current LUFS loudness readings.
    /// </summary>
    /// <param name="momentary">Momentary loudness (400ms window)</param>
    /// <param name="shortTerm">Short-term loudness (3s window)</param>
    /// <param name="integrated">Integrated loudness (gated)</param>
    /// <returns>True on success</returns>
    public bool GetLufs(out float momentary, out float shortTerm, out float integrated)
    {
        if (_handle == IntPtr.Zero)
        {
            momentary = shortTerm = integrated = -100f;
            return false;
        }
        return BassProcessorNative.BASS_MultibandProcessor_GetLUFS(_handle, out momentary, out shortTerm, out integrated) != 0;
    }

    /// <summary>
    /// Reset LUFS meter measurements.
    /// </summary>
    public bool ResetLufs()
    {
        if (_handle == IntPtr.Zero) return false;
        return BassProcessorNative.BASS_MultibandProcessor_ResetLUFS(_handle) != 0;
    }

    /// <summary>
    /// Gets or sets whether LUFS metering is enabled.
    /// </summary>
    public bool IsLufsEnabled
    {
        get => _handle != IntPtr.Zero &&
            BassProcessorNative.BASS_MultibandProcessor_IsLUFSEnabled(_handle) != 0;
        set
        {
            if (_handle != IntPtr.Zero)
                BassProcessorNative.BASS_MultibandProcessor_SetLUFSEnabled(_handle, value ? 1 : 0);
        }
    }

    // =========================================================================
    // Dispose
    // =========================================================================

    /// <summary>
    /// Dispose of the processor and release all resources.
    /// </summary>
    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;

        // Stop stats callback first
        if (_handle != IntPtr.Zero && _statsDelegate != null)
        {
            BassProcessorNative.BASS_MultibandProcessor_SetStatsCallback(
                _handle, null, 0, IntPtr.Zero);
        }

        // Free the processor
        if (_handle != IntPtr.Zero)
        {
            BassProcessorNative.BASS_MultibandProcessor_Free(_handle);
            _handle = IntPtr.Zero;
        }

        // Clear delegate reference
        _statsDelegate = null;

        GC.SuppressFinalize(this);
    }

    ~BroadcastProcessor() => Dispose();
}
