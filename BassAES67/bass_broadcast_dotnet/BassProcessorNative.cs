using System.Runtime.InteropServices;

namespace BassProcessor;

/// <summary>
/// P/Invoke declarations for bass_broadcast_processor plugin.
/// Matches the FFI definitions in bass_broadcast_processor/src/lib.rs
/// </summary>
public static class BassProcessorNative
{
    // =========================================================================
    // Constants
    // =========================================================================

    /// <summary>Maximum number of bands supported in stats callback</summary>
    public const int MAX_BANDS = 8;

    /// <summary>AGC mode: single-stage</summary>
    public const byte AGC_MODE_SINGLE = 0;

    /// <summary>AGC mode: 3-stage cascaded (Omnia 9 style)</summary>
    public const byte AGC_MODE_THREE_STAGE = 1;

    /// <summary>Soft clipper mode: hard clipping</summary>
    public const byte CLIP_MODE_HARD = 0;

    /// <summary>Soft clipper mode: soft knee clipping</summary>
    public const byte CLIP_MODE_SOFT = 1;

    /// <summary>Soft clipper mode: tanh saturation</summary>
    public const byte CLIP_MODE_TANH = 2;

    // =========================================================================
    // Configuration Structures
    // =========================================================================

    /// <summary>
    /// Compressor configuration (per-band).
    /// </summary>
    [StructLayout(LayoutKind.Sequential)]
    public struct CompressorConfig
    {
        /// <summary>Threshold in dBFS (-40.0 to 0.0)</summary>
        public float ThresholdDb;

        /// <summary>Compression ratio (1.0 = no compression, 10.0 = heavy)</summary>
        public float Ratio;

        /// <summary>Attack time in milliseconds (0.5 to 100)</summary>
        public float AttackMs;

        /// <summary>Release time in milliseconds (10 to 1000)</summary>
        public float ReleaseMs;

        /// <summary>Makeup gain in dB (0.0 to 20.0)</summary>
        public float MakeupGainDb;

        /// <summary>Lookahead time in milliseconds (0.0 to 10.0)</summary>
        public float LookaheadMs;

        public static CompressorConfig Default => new()
        {
            ThresholdDb = -20.0f,
            Ratio = 4.0f,
            AttackMs = 10.0f,
            ReleaseMs = 100.0f,
            MakeupGainDb = 0.0f,
            LookaheadMs = 0.0f
        };
    }

    /// <summary>
    /// Multiband processor configuration header.
    /// </summary>
    [StructLayout(LayoutKind.Sequential)]
    public struct MultibandConfigHeader
    {
        /// <summary>Sample rate in Hz (typically 48000)</summary>
        public uint SampleRate;

        /// <summary>Number of channels (2 for stereo)</summary>
        public ushort Channels;

        /// <summary>Number of frequency bands (2, 5, 8, etc.)</summary>
        public ushort NumBands;

        /// <summary>If non-zero, output is decode-only (for feeding to AES67)</summary>
        public byte DecodeOutput;

        /// <summary>Padding for alignment</summary>
        private byte _pad1;
        private byte _pad2;
        private byte _pad3;

        /// <summary>Input gain in dB (-20.0 to +20.0)</summary>
        public float InputGainDb;

        /// <summary>Output gain in dB (-20.0 to +20.0)</summary>
        public float OutputGainDb;

        public static MultibandConfigHeader Default => new()
        {
            SampleRate = 48000,
            Channels = 2,
            NumBands = 5,
            DecodeOutput = 0,
            InputGainDb = 0.0f,
            OutputGainDb = 0.0f
        };

        /// <summary>
        /// Create a configuration header with int/bool parameters.
        /// </summary>
        public static MultibandConfigHeader Create(
            int sampleRate = 48000,
            int channels = 2,
            int numBands = 5,
            bool decodeOutput = false,
            float inputGainDb = 0.0f,
            float outputGainDb = 0.0f) => new()
        {
            SampleRate = (uint)sampleRate,
            Channels = (ushort)channels,
            NumBands = (ushort)numBands,
            DecodeOutput = decodeOutput ? (byte)1 : (byte)0,
            InputGainDb = inputGainDb,
            OutputGainDb = outputGainDb
        };

        /// <summary>Gets or sets whether output is decode-only (for feeding to AES67).</summary>
        public bool IsDecodeOutput
        {
            readonly get => DecodeOutput != 0;
            set => DecodeOutput = value ? (byte)1 : (byte)0;
        }
    }

    /// <summary>
    /// AGC (Automatic Gain Control) configuration.
    /// </summary>
    [StructLayout(LayoutKind.Sequential)]
    public struct AgcConfig
    {
        /// <summary>Target output level in dBFS (-24.0 to -12.0)</summary>
        public float TargetLevelDb;

        /// <summary>Compression threshold in dBFS (-30.0 to -6.0)</summary>
        public float ThresholdDb;

        /// <summary>Compression ratio (2.0 to 8.0)</summary>
        public float Ratio;

        /// <summary>Soft knee width in dB (0.0 to 20.0)</summary>
        public float KneeDb;

        /// <summary>Attack time in milliseconds</summary>
        public float AttackMs;

        /// <summary>Release time in milliseconds</summary>
        public float ReleaseMs;

        /// <summary>Enable flag (1 = enabled, 0 = bypassed)</summary>
        public byte Enabled;

        /// <summary>AGC mode: 0 = single-stage, 1 = 3-stage</summary>
        public byte Mode;

        /// <summary>Padding for alignment</summary>
        private byte _pad1;
        private byte _pad2;

        public static AgcConfig Default => new()
        {
            TargetLevelDb = -18.0f,
            ThresholdDb = -24.0f,
            Ratio = 3.0f,
            KneeDb = 10.0f,
            AttackMs = 50.0f,
            ReleaseMs = 500.0f,
            Enabled = 1,
            Mode = AGC_MODE_SINGLE
        };

        /// <summary>
        /// Create an AGC configuration with bool/int parameters.
        /// </summary>
        public static AgcConfig Create(
            float targetLevelDb = -18.0f,
            float thresholdDb = -24.0f,
            float ratio = 3.0f,
            float kneeDb = 10.0f,
            float attackMs = 50.0f,
            float releaseMs = 500.0f,
            bool enabled = true,
            bool threeStageMode = false) => new()
        {
            TargetLevelDb = targetLevelDb,
            ThresholdDb = thresholdDb,
            Ratio = ratio,
            KneeDb = kneeDb,
            AttackMs = attackMs,
            ReleaseMs = releaseMs,
            Enabled = enabled ? (byte)1 : (byte)0,
            Mode = threeStageMode ? AGC_MODE_THREE_STAGE : AGC_MODE_SINGLE
        };

        /// <summary>Gets or sets whether this AGC is enabled.</summary>
        public bool IsEnabled
        {
            readonly get => Enabled != 0;
            set => Enabled = value ? (byte)1 : (byte)0;
        }
    }

    /// <summary>
    /// 3-stage cascaded AGC configuration (Omnia 9 style).
    /// </summary>
    [StructLayout(LayoutKind.Sequential)]
    public struct Agc3StageConfig
    {
        /// <summary>Stage 1: Slow AGC for song-level normalization</summary>
        public AgcConfig Slow;

        /// <summary>Stage 2: Medium AGC for phrase-level dynamics</summary>
        public AgcConfig Medium;

        /// <summary>Stage 3: Fast AGC for syllable/transient control</summary>
        public AgcConfig Fast;
    }

    /// <summary>
    /// Per-band stereo enhancer configuration.
    /// </summary>
    [StructLayout(LayoutKind.Sequential)]
    public struct StereoEnhancerBandConfig
    {
        /// <summary>Target stereo width ratio (0.0 = mono, 1.0 = natural, 2.0 = enhanced)</summary>
        public float TargetWidth;

        /// <summary>Maximum gain boost to side signal in dB (0.0 to 18.0)</summary>
        public float MaxGainDb;

        /// <summary>Maximum attenuation to side signal in dB (0.0 to 18.0)</summary>
        public float MaxAttenDb;

        /// <summary>Attack time in ms (1.0 to 200.0)</summary>
        public float AttackMs;

        /// <summary>Release time in ms (10.0 to 500.0)</summary>
        public float ReleaseMs;

        /// <summary>Enable flag (1 = enabled, 0 = bypassed)</summary>
        public byte Enabled;

        /// <summary>Padding for alignment</summary>
        private byte _pad1;
        private byte _pad2;
        private byte _pad3;

        /// <summary>Gets or sets whether this band is enabled.</summary>
        public bool IsEnabled
        {
            readonly get => Enabled != 0;
            set => Enabled = value ? (byte)1 : (byte)0;
        }
    }

    /// <summary>
    /// Multiband stereo enhancer configuration.
    /// </summary>
    [StructLayout(LayoutKind.Sequential)]
    public struct StereoEnhancerConfig
    {
        /// <summary>Global enable flag (1 = enabled, 0 = bypassed)</summary>
        public byte Enabled;

        /// <summary>Padding for alignment</summary>
        private byte _pad1;
        private byte _pad2;
        private byte _pad3;

        /// <summary>Per-band configurations (5 bands)</summary>
        [MarshalAs(UnmanagedType.ByValArray, SizeConst = 5)]
        public StereoEnhancerBandConfig[] Bands;

        /// <summary>Gets or sets whether stereo enhancer is globally enabled.</summary>
        public bool IsEnabled
        {
            readonly get => Enabled != 0;
            set => Enabled = value ? (byte)1 : (byte)0;
        }
    }

    /// <summary>
    /// Per-band parametric EQ configuration.
    /// </summary>
    [StructLayout(LayoutKind.Sequential)]
    public struct ParametricEqBandConfig
    {
        /// <summary>Center frequency in Hz (20.0 to 20000.0)</summary>
        public float Frequency;

        /// <summary>Q factor (0.1 to 10.0, higher = narrower bandwidth)</summary>
        public float Q;

        /// <summary>Gain in dB (-12.0 to +12.0)</summary>
        public float GainDb;

        /// <summary>Enable flag (1 = enabled, 0 = bypassed)</summary>
        public byte Enabled;

        /// <summary>Padding for alignment</summary>
        private byte _pad1;
        private byte _pad2;
        private byte _pad3;

        /// <summary>Gets or sets whether this EQ band is enabled.</summary>
        public bool IsEnabled
        {
            readonly get => Enabled != 0;
            set => Enabled = value ? (byte)1 : (byte)0;
        }
    }

    /// <summary>
    /// Full parametric EQ configuration for 5 bands.
    /// </summary>
    [StructLayout(LayoutKind.Sequential)]
    public struct ParametricEqConfig
    {
        /// <summary>Global enable flag (1 = enabled, 0 = bypassed)</summary>
        public byte Enabled;

        /// <summary>Padding for alignment</summary>
        private byte _pad1;
        private byte _pad2;
        private byte _pad3;

        /// <summary>Per-band EQ settings (5 bands)</summary>
        [MarshalAs(UnmanagedType.ByValArray, SizeConst = 5)]
        public ParametricEqBandConfig[] Bands;

        /// <summary>Gets or sets whether parametric EQ is globally enabled.</summary>
        public bool IsEnabled
        {
            readonly get => Enabled != 0;
            set => Enabled = value ? (byte)1 : (byte)0;
        }
    }

    /// <summary>
    /// Soft clipper configuration.
    /// </summary>
    [StructLayout(LayoutKind.Sequential)]
    public struct SoftClipperConfig
    {
        /// <summary>Ceiling level in dBFS (-6.0 to 0.0)</summary>
        public float CeilingDb;

        /// <summary>Knee width in dB (0.0 to 6.0, only for soft mode)</summary>
        public float KneeDb;

        /// <summary>Clipping mode: 0=hard, 1=soft, 2=tanh</summary>
        public byte Mode;

        /// <summary>Oversampling factor: 1, 2, or 4</summary>
        public byte Oversample;

        /// <summary>Enable flag (1 = enabled, 0 = bypassed)</summary>
        public byte Enabled;

        /// <summary>Padding for alignment</summary>
        private byte _pad;

        public static SoftClipperConfig Default => new()
        {
            CeilingDb = -0.1f,
            KneeDb = 3.0f,
            Mode = CLIP_MODE_SOFT,
            Oversample = 1,
            Enabled = 0
        };

        /// <summary>Gets or sets whether soft clipper is enabled.</summary>
        public bool IsEnabled
        {
            readonly get => Enabled != 0;
            set => Enabled = value ? (byte)1 : (byte)0;
        }
    }

    /// <summary>
    /// Multiband processor statistics header.
    /// </summary>
    [StructLayout(LayoutKind.Sequential)]
    public struct MultibandStatsHeader
    {
        /// <summary>Total samples (frames) processed</summary>
        public ulong SamplesProcessed;

        /// <summary>Input peak level (linear, 0.0 to 1.0+)</summary>
        public float InputPeak;

        /// <summary>Output peak level (linear, 0.0 to 1.0+)</summary>
        public float OutputPeak;

        /// <summary>Number of bands</summary>
        public uint NumBands;

        /// <summary>AGC gain reduction in dB</summary>
        public float AgcGrDb;

        /// <summary>Number of source underruns</summary>
        public ulong Underruns;

        /// <summary>Last processing time in microseconds</summary>
        public ulong ProcessTimeUs;

        /// <summary>Momentary loudness (LUFS, 400ms window)</summary>
        public float LufsMomentary;

        /// <summary>Short-term loudness (LUFS, 3s window)</summary>
        public float LufsShortTerm;

        /// <summary>Integrated loudness (LUFS, gated)</summary>
        public float LufsIntegrated;

        /// <summary>Padding for alignment</summary>
        private uint _pad;
    }

    // =========================================================================
    // Stats Callback
    // =========================================================================

    /// <summary>
    /// Stats callback data structure.
    /// Contains all real-time statistics in a fixed-size structure.
    /// </summary>
    [StructLayout(LayoutKind.Sequential)]
    public struct ProcessorStatsCallbackData
    {
        /// <summary>Momentary loudness (LUFS, 400ms window)</summary>
        public float LufsMomentary;

        /// <summary>Short-term loudness (LUFS, 3s window)</summary>
        public float LufsShortTerm;

        /// <summary>Integrated loudness (LUFS, gated)</summary>
        public float LufsIntegrated;

        /// <summary>Input peak level (linear, 0.0 to 1.0+)</summary>
        public float InputPeak;

        /// <summary>Output peak level (linear, 0.0 to 1.0+)</summary>
        public float OutputPeak;

        /// <summary>AGC gain reduction in dB</summary>
        public float AgcGrDb;

        /// <summary>Per-band gain reduction in dB (fixed 8-element array)</summary>
        [MarshalAs(UnmanagedType.ByValArray, SizeConst = MAX_BANDS)]
        public float[] BandGrDb;

        /// <summary>Actual number of bands in use (1-8)</summary>
        public uint NumBands;

        /// <summary>Clipper activity (0.0 = no clipping, 1.0 = constant clipping)</summary>
        public float ClipperActivity;

        /// <summary>Total samples (frames) processed</summary>
        public ulong SamplesProcessed;

        /// <summary>Number of source underruns</summary>
        public ulong Underruns;

        /// <summary>Last processing time in microseconds</summary>
        public ulong ProcessTimeUs;
    }

    /// <summary>
    /// Delegate for stats callback.
    /// </summary>
    /// <param name="stats">Pointer to statistics data</param>
    /// <param name="user">User data pointer</param>
    [UnmanagedFunctionPointer(CallingConvention.StdCall)]
    public delegate void OnStatsCallback(ref ProcessorStatsCallbackData stats, IntPtr user);

    // =========================================================================
    // Core Multiband Processor API
    // =========================================================================

    /// <summary>
    /// Create a new N-band multiband processor.
    /// </summary>
    /// <param name="sourceChannel">BASS channel handle to pull audio from</param>
    /// <param name="header">Pointer to MultibandConfigHeader structure</param>
    /// <param name="crossoverFreqs">Pointer to array of crossover frequencies (num_bands - 1 elements)</param>
    /// <param name="bands">Pointer to array of CompressorConfig (num_bands elements)</param>
    /// <returns>Opaque handle or IntPtr.Zero on failure</returns>
    [DllImport("bass_broadcast_processor", CallingConvention = CallingConvention.StdCall)]
    public static extern IntPtr BASS_MultibandProcessor_Create(
        int sourceChannel,
        ref MultibandConfigHeader header,
        [In] float[] crossoverFreqs,
        [In] CompressorConfig[] bands);

    /// <summary>
    /// Get the output BASS stream handle.
    /// </summary>
    [DllImport("bass_broadcast_processor", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_MultibandProcessor_GetOutput(IntPtr handle);

    /// <summary>
    /// Free the processor and associated BASS stream.
    /// </summary>
    [DllImport("bass_broadcast_processor", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_MultibandProcessor_Free(IntPtr handle);

    /// <summary>
    /// Set bypass mode.
    /// </summary>
    [DllImport("bass_broadcast_processor", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_MultibandProcessor_SetBypass(IntPtr handle, int bypass);

    /// <summary>
    /// Set input and output gains.
    /// </summary>
    [DllImport("bass_broadcast_processor", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_MultibandProcessor_SetGains(IntPtr handle, float inputGainDb, float outputGainDb);

    /// <summary>
    /// Reset processor state.
    /// </summary>
    [DllImport("bass_broadcast_processor", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_MultibandProcessor_Reset(IntPtr handle);

    /// <summary>
    /// Pre-fill the processor buffer.
    /// </summary>
    [DllImport("bass_broadcast_processor", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_MultibandProcessor_Prefill(IntPtr handle);

    /// <summary>
    /// Get the number of bands.
    /// </summary>
    [DllImport("bass_broadcast_processor", CallingConvention = CallingConvention.StdCall)]
    public static extern uint BASS_MultibandProcessor_GetNumBands(IntPtr handle);

    /// <summary>
    /// Update a specific band's compressor settings.
    /// </summary>
    [DllImport("bass_broadcast_processor", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_MultibandProcessor_SetBand(IntPtr handle, uint band, ref CompressorConfig config);

    /// <summary>
    /// Get processor statistics.
    /// </summary>
    [DllImport("bass_broadcast_processor", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_MultibandProcessor_GetStats(IntPtr handle, out MultibandStatsHeader header, [Out] float[] bandGr);

    /// <summary>
    /// Set or clear the stats callback for periodic updates.
    /// </summary>
    [DllImport("bass_broadcast_processor", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_MultibandProcessor_SetStatsCallback(
        IntPtr handle,
        OnStatsCallback? callback,
        uint intervalMs,
        IntPtr user);

    // =========================================================================
    // Lookahead Control
    // =========================================================================

    /// <summary>
    /// Set lookahead for all compressor bands.
    /// </summary>
    [DllImport("bass_broadcast_processor", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_MultibandProcessor_SetLookahead(IntPtr handle, int enabled, float lookaheadMs);

    /// <summary>
    /// Get current lookahead latency in milliseconds.
    /// </summary>
    [DllImport("bass_broadcast_processor", CallingConvention = CallingConvention.StdCall)]
    public static extern float BASS_MultibandProcessor_GetLookahead(IntPtr handle);

    // =========================================================================
    // AGC (Automatic Gain Control)
    // =========================================================================

    /// <summary>
    /// Set AGC parameters.
    /// </summary>
    [DllImport("bass_broadcast_processor", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_MultibandProcessor_SetAGC(IntPtr handle, ref AgcConfig config);

    /// <summary>
    /// Get default AGC configuration.
    /// </summary>
    [DllImport("bass_broadcast_processor", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_MultibandProcessor_GetDefaultAGC(out AgcConfig config);

    /// <summary>
    /// Set 3-stage AGC configuration.
    /// </summary>
    [DllImport("bass_broadcast_processor", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_MultibandProcessor_SetAGC3Stage(IntPtr handle, ref Agc3StageConfig config);

    /// <summary>
    /// Get default 3-stage AGC configuration.
    /// </summary>
    [DllImport("bass_broadcast_processor", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_MultibandProcessor_GetDefaultAGC3Stage(out Agc3StageConfig config);

    /// <summary>
    /// Check if 3-stage AGC mode is active.
    /// </summary>
    [DllImport("bass_broadcast_processor", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_MultibandProcessor_IsAGC3Stage(IntPtr handle);

    /// <summary>
    /// Get individual stage gain reduction values.
    /// </summary>
    [DllImport("bass_broadcast_processor", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_MultibandProcessor_GetAGC3StageGR(IntPtr handle, out float slowGr, out float mediumGr, out float fastGr);

    // =========================================================================
    // Stereo Enhancer
    // =========================================================================

    /// <summary>
    /// Set stereo enhancer configuration.
    /// </summary>
    [DllImport("bass_broadcast_processor", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_MultibandProcessor_SetStereoEnhancer(IntPtr handle, ref StereoEnhancerConfig config);

    /// <summary>
    /// Get default stereo enhancer configuration.
    /// </summary>
    [DllImport("bass_broadcast_processor", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_MultibandProcessor_GetDefaultStereoEnhancer(out StereoEnhancerConfig config);

    /// <summary>
    /// Check if stereo enhancer is enabled.
    /// </summary>
    [DllImport("bass_broadcast_processor", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_MultibandProcessor_IsStereoEnhancerEnabled(IntPtr handle);

    /// <summary>
    /// Enable or disable stereo enhancer.
    /// </summary>
    [DllImport("bass_broadcast_processor", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_MultibandProcessor_SetStereoEnhancerEnabled(IntPtr handle, int enabled);

    // =========================================================================
    // Parametric EQ
    // =========================================================================

    /// <summary>
    /// Set parametric EQ configuration.
    /// </summary>
    [DllImport("bass_broadcast_processor", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_MultibandProcessor_SetParametricEQ(IntPtr handle, ref ParametricEqConfig config);

    /// <summary>
    /// Get default parametric EQ configuration.
    /// </summary>
    [DllImport("bass_broadcast_processor", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_MultibandProcessor_GetDefaultParametricEQ(out ParametricEqConfig config);

    /// <summary>
    /// Check if parametric EQ is enabled.
    /// </summary>
    [DllImport("bass_broadcast_processor", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_MultibandProcessor_IsParametricEQEnabled(IntPtr handle);

    /// <summary>
    /// Enable or disable parametric EQ.
    /// </summary>
    [DllImport("bass_broadcast_processor", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_MultibandProcessor_SetParametricEQEnabled(IntPtr handle, int enabled);

    // =========================================================================
    // Soft Clipper
    // =========================================================================

    /// <summary>
    /// Set soft clipper configuration.
    /// </summary>
    [DllImport("bass_broadcast_processor", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_MultibandProcessor_SetSoftClipper(IntPtr handle, ref SoftClipperConfig config);

    /// <summary>
    /// Get default soft clipper configuration.
    /// </summary>
    [DllImport("bass_broadcast_processor", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_MultibandProcessor_GetDefaultSoftClipper(out SoftClipperConfig config);

    /// <summary>
    /// Check if soft clipper is enabled.
    /// </summary>
    [DllImport("bass_broadcast_processor", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_MultibandProcessor_IsSoftClipperEnabled(IntPtr handle);

    /// <summary>
    /// Enable or disable soft clipper.
    /// </summary>
    [DllImport("bass_broadcast_processor", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_MultibandProcessor_SetSoftClipperEnabled(IntPtr handle, int enabled);

    /// <summary>
    /// Get soft clipper latency in milliseconds.
    /// </summary>
    [DllImport("bass_broadcast_processor", CallingConvention = CallingConvention.StdCall)]
    public static extern float BASS_MultibandProcessor_GetSoftClipperLatency(IntPtr handle);

    // =========================================================================
    // LUFS Metering
    // =========================================================================

    /// <summary>
    /// Get LUFS loudness readings.
    /// </summary>
    [DllImport("bass_broadcast_processor", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_MultibandProcessor_GetLUFS(IntPtr handle, out float momentary, out float shortTerm, out float integrated);

    /// <summary>
    /// Reset LUFS meter measurements.
    /// </summary>
    [DllImport("bass_broadcast_processor", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_MultibandProcessor_ResetLUFS(IntPtr handle);

    /// <summary>
    /// Check if LUFS metering is enabled.
    /// </summary>
    [DllImport("bass_broadcast_processor", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_MultibandProcessor_IsLUFSEnabled(IntPtr handle);

    /// <summary>
    /// Enable or disable LUFS metering.
    /// </summary>
    [DllImport("bass_broadcast_processor", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_MultibandProcessor_SetLUFSEnabled(IntPtr handle, int enabled);
}
