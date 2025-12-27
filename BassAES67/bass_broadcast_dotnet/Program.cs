using Un4seen.Bass;
using BassProcessor;

namespace BassProcessorDemo;

/// <summary>
/// Demo program showing how to use the BroadcastProcessor with all features.
/// </summary>
class Program
{
    static BroadcastProcessor? _processor;
    static int _sourceChannel;
    static bool _running = true;

    static void Main(string[] args)
    {
        Console.WriteLine("=== Bass Broadcast Processor Demo ===\n");

        // Initialize BASS
        if (!Bass.BASS_Init(-1, 48000, BASSInit.BASS_DEVICE_DEFAULT, IntPtr.Zero))
        {
            Console.WriteLine($"BASS_Init failed: {Bass.BASS_ErrorGetCode()}");
            return;
        }

        // Load a test file (change this path to your audio file)
        //string testFile = args.Length > 0 ? args[0] : @"F:\Audio\GlobalNewsPodcast-20251215.mp3";
string testFile = args.Length > 0 ? args[0] : @"E:\PromoOnly2022\Mainstream Radio August 2022\KateBush-RunningUpThatHill(ADealWithGod)(RadioEdit)-(m4a).m4a";
//
        _sourceChannel = Bass.BASS_StreamCreateFile(testFile, 0, 0,
            BASSFlag.BASS_STREAM_DECODE | BASSFlag.BASS_SAMPLE_FLOAT);

        if (_sourceChannel == 0)
        {
            Console.WriteLine($"Failed to load '{testFile}': {Bass.BASS_ErrorGetCode()}");
            Console.WriteLine("Usage: BassProcessor <audio_file.mp3>");
            Bass.BASS_Free();
            return;
        }

        Console.WriteLine($"Loaded: {testFile}\n");

        // =========================================================================
        // 1. CREATE THE PROCESSOR
        // =========================================================================

        // Option A: Use factory method for 5-band (recommended)
        _processor = BroadcastProcessor.Create5Band(_sourceChannel, decodeOutput: false);

        // Option B: Use factory method for 2-band (lower latency)
        // _processor = BroadcastProcessor.Create2Band(_sourceChannel);

        // Option C: Custom configuration
        // var header = MultibandConfigHeader.Create(sampleRate: 48000, numBands: 5);
        // float[] crossovers = [120f, 400f, 2000f, 8000f];
        // var bands = new CompressorConfig[] { ... };
        // _processor = new BroadcastProcessor(_sourceChannel, header, crossovers, bands);

        Console.WriteLine($"Created {_processor.NumBands}-band processor\n");

        // =========================================================================
        // 2. SUBSCRIBE TO STATS CALLBACK
        // =========================================================================

        _processor.StatsUpdated += OnStatsUpdated;
        _processor.EnableStats(100); // 100ms interval (10 updates per second)

        // =========================================================================
        // 3. CONFIGURE EACH PROCESSING STAGE
        // =========================================================================

        ConfigureAgc();
        ConfigureStereoEnhancer();
        ConfigureParametricEq();
        ConfigureSoftClipper();
        ConfigureLufs();

        // =========================================================================
        // 4. START PLAYBACK
        // =========================================================================

        int outputHandle = _processor.OutputHandle;
        Bass.BASS_ChannelPlay(outputHandle, false);

        Console.WriteLine("Playing... Press keys to control:\n");
        Console.WriteLine("  [B] Toggle Bypass");
        Console.WriteLine("  [A] Toggle AGC");
        Console.WriteLine("  [S] Toggle Stereo Enhancer");
        Console.WriteLine("  [E] Toggle Parametric EQ");
        Console.WriteLine("  [C] Toggle Soft Clipper");
        Console.WriteLine("  [L] Toggle LUFS Metering");
        Console.WriteLine("  [+] Increase Output Gain");
        Console.WriteLine("  [-] Decrease Output Gain");
        Console.WriteLine("  [R] Reset Processor");
        Console.WriteLine("  [Q] Quit\n");

        // =========================================================================
        // 5. INTERACTIVE CONTROL LOOP
        // =========================================================================

        float outputGain = 0f;
        bool bypass = false;

        while (_running)
        {
            if (Console.KeyAvailable)
            {
                var key = Console.ReadKey(true).Key;

                switch (key)
                {
                    case ConsoleKey.B:
                        bypass = !bypass;
                        _processor.SetBypass(bypass);
                        Console.WriteLine($"\n>>> Bypass: {(bypass ? "ON" : "OFF")}");
                        break;

                    case ConsoleKey.A:
                        ToggleAgc();
                        break;

                    case ConsoleKey.S:
                        _processor.IsStereoEnhancerEnabled = !_processor.IsStereoEnhancerEnabled;
                        Console.WriteLine($"\n>>> Stereo Enhancer: {(_processor.IsStereoEnhancerEnabled ? "ON" : "OFF")}");
                        break;

                    case ConsoleKey.E:
                        _processor.IsParametricEqEnabled = !_processor.IsParametricEqEnabled;
                        Console.WriteLine($"\n>>> Parametric EQ: {(_processor.IsParametricEqEnabled ? "ON" : "OFF")}");
                        break;

                    case ConsoleKey.C:
                        _processor.IsSoftClipperEnabled = !_processor.IsSoftClipperEnabled;
                        Console.WriteLine($"\n>>> Soft Clipper: {(_processor.IsSoftClipperEnabled ? "ON" : "OFF")}");
                        break;

                    case ConsoleKey.L:
                        _processor.IsLufsEnabled = !_processor.IsLufsEnabled;
                        Console.WriteLine($"\n>>> LUFS Metering: {(_processor.IsLufsEnabled ? "ON" : "OFF")}");
                        break;

                    case ConsoleKey.Add:
                    case ConsoleKey.OemPlus:
                        outputGain = Math.Min(outputGain + 1f, 12f);
                        _processor.SetGains(0f, outputGain);
                        Console.WriteLine($"\n>>> Output Gain: {outputGain:+0.0;-0.0;0.0} dB");
                        break;

                    case ConsoleKey.Subtract:
                    case ConsoleKey.OemMinus:
                        outputGain = Math.Max(outputGain - 1f, -12f);
                        _processor.SetGains(0f, outputGain);
                        Console.WriteLine($"\n>>> Output Gain: {outputGain:+0.0;-0.0;0.0} dB");
                        break;

                    case ConsoleKey.R:
                        _processor.Reset();
                        _processor.ResetLufs();
                        Console.WriteLine("\n>>> Processor Reset");
                        break;

                    case ConsoleKey.Q:
                        _running = false;
                        break;
                }
            }

            // Check if playback ended
            if (Bass.BASS_ChannelIsActive(outputHandle) != BASSActive.BASS_ACTIVE_PLAYING)
            {
                Console.WriteLine("\n\nPlayback finished.");
                break;
            }

            Thread.Sleep(50);
        }

        // =========================================================================
        // 6. CLEANUP
        // =========================================================================

        Console.WriteLine("\nCleaning up...");

        _processor.DisableStats();
        _processor.Dispose();
        Bass.BASS_StreamFree(_sourceChannel);
        Bass.BASS_Free();

        Console.WriteLine("Done.");
    }

    /// <summary>
    /// Stats callback - fires every 100ms with real-time metering data.
    /// </summary>
    static void OnStatsUpdated(ProcessorStats stats)
    {
        // Build a compact status line
        string lufs = stats.LufsMomentary > -100
            ? $"{stats.LufsMomentary,6:F1}"
            : "  -inf";

        string inPeak = stats.InputPeakDbfs > -100
            ? $"{stats.InputPeakDbfs,6:F1}"
            : "  -inf";

        string outPeak = stats.OutputPeakDbfs > -100
            ? $"{stats.OutputPeakDbfs,6:F1}"
            : "  -inf";

        // Per-band gain reduction (show first 5 bands)
        string bandGr = "";
        for (int i = 0; i < Math.Min(stats.NumBands, 5); i++)
        {
            bandGr += $"{stats.BandGrDb[i],5:F1} ";
        }

        Console.Write($"\rLUFS:{lufs} | In:{inPeak} Out:{outPeak} dBFS | AGC:{stats.AgcGrDb,5:F1}dB | Bands:[{bandGr}] | {stats.ProcessTime.TotalMicroseconds,4:F0}us   ");
    }

    // =========================================================================
    // CONFIGURATION EXAMPLES
    // =========================================================================

    /// <summary>
    /// Configure AGC (Automatic Gain Control).
    /// </summary>
    static void ConfigureAgc()
    {
        // Option 1: Use defaults
        var agc = BassProcessorNative.AgcConfig.Default;

        // Option 2: Use Create() helper with named parameters
        // var agc = BassProcessorNative.AgcConfig.Create(
        //     targetLevelDb: -16f,
        //     thresholdDb: -20f,
        //     ratio: 4f,
        //     attackMs: 30f,
        //     releaseMs: 300f,
        //     enabled: true
        // );

        // Option 3: Modify individual properties
        agc.TargetLevelDb = -16f;
        agc.IsEnabled = true;

        _processor!.SetAgc(agc);
        Console.WriteLine("AGC: Enabled (target -16 LUFS)");
    }

    /// <summary>
    /// Toggle AGC and demonstrate changing values on the fly.
    /// </summary>
    static void ToggleAgc()
    {
        // Get current state and toggle
        var agc = BassProcessorNative.AgcConfig.Default;

        // Example: cycle through different AGC targets
        float[] targets = [-14f, -16f, -18f, -20f];
        int currentIndex = Array.IndexOf(targets, agc.TargetLevelDb);
        int nextIndex = (currentIndex + 1) % targets.Length;

        agc.TargetLevelDb = targets[nextIndex];
        agc.IsEnabled = true;

        _processor!.SetAgc(agc);
        Console.WriteLine($"\n>>> AGC Target: {agc.TargetLevelDb:F0} dBFS");
    }

    /// <summary>
    /// Configure Stereo Enhancer.
    /// </summary>
    static void ConfigureStereoEnhancer()
    {
        // Get default configuration
        BassProcessorNative.BASS_MultibandProcessor_GetDefaultStereoEnhancer(
            out var config);

        // Enable globally
        config.IsEnabled = true;

        // Configure per-band widths (if Bands array is initialized)
        if (config.Bands != null && config.Bands.Length >= 5)
        {
            // Low frequencies: keep narrow (mono-compatible bass)
            config.Bands[0].TargetWidth = 0.8f;
            config.Bands[0].IsEnabled = true;

            // Mid frequencies: natural width
            config.Bands[1].TargetWidth = 1.0f;
            config.Bands[1].IsEnabled = true;

            config.Bands[2].TargetWidth = 1.2f;
            config.Bands[2].IsEnabled = true;

            // High frequencies: wider for "air"
            config.Bands[3].TargetWidth = 1.4f;
            config.Bands[3].IsEnabled = true;

            config.Bands[4].TargetWidth = 1.5f;
            config.Bands[4].IsEnabled = true;
        }

        _processor!.SetStereoEnhancer(config);
        _processor.IsStereoEnhancerEnabled = false; // Start disabled
        Console.WriteLine("Stereo Enhancer: Configured (press S to enable)");
    }

    /// <summary>
    /// Configure Parametric EQ.
    /// </summary>
    static void ConfigureParametricEq()
    {
        // Get default configuration
        BassProcessorNative.BASS_MultibandProcessor_GetDefaultParametricEQ(
            out var config);

        config.IsEnabled = true;

        // Configure 5-band parametric EQ
        if (config.Bands != null && config.Bands.Length >= 5)
        {
            // Band 1: Low shelf boost at 80Hz
            config.Bands[0].Frequency = 70f;
            config.Bands[0].Q = 0.7f;
            config.Bands[0].GainDb = 8f;
            config.Bands[0].IsEnabled = true;

            // Band 2: Cut mud at 250Hz
            config.Bands[1].Frequency = 250f;
            config.Bands[1].Q = 1.5f;
            config.Bands[1].GainDb = -2f;
            config.Bands[1].IsEnabled = true;

            // Band 3: Presence boost at 3kHz
            config.Bands[2].Frequency = 3000f;
            config.Bands[2].Q = 1.0f;
            config.Bands[2].GainDb = 1.5f;
            config.Bands[2].IsEnabled = true;

            // Band 4: Air at 12kHz
            config.Bands[3].Frequency = 6000f;
            config.Bands[3].Q = 0.7f;
            config.Bands[3].GainDb = 8f;
            config.Bands[3].IsEnabled = true;

            // Band 5: Not used
            config.Bands[4].IsEnabled = false;
        }

        _processor!.SetParametricEq(config);
        _processor.IsParametricEqEnabled = false; // Start disabled
        Console.WriteLine("Parametric EQ: Configured (press E to enable)");
    }

    /// <summary>
    /// Configure Soft Clipper.
    /// </summary>
    static void ConfigureSoftClipper()
    {
        var clipper = BassProcessorNative.SoftClipperConfig.Default;

        // Ceiling just below 0 dBFS to prevent digital clipping
        clipper.CeilingDb = -0.3f;

        // Soft knee for transparent limiting
        clipper.KneeDb = 3f;

        // Clipping modes:
        // CLIP_MODE_HARD = 0 (hard clip - harsh)
        // CLIP_MODE_SOFT = 1 (soft knee - recommended)
        // CLIP_MODE_TANH = 2 (tanh saturation - warmest)
        clipper.Mode = BassProcessorNative.CLIP_MODE_SOFT;

        // Oversampling: 1, 2, or 4x
        // Higher = cleaner but more CPU
        clipper.Oversample = 2;

        clipper.IsEnabled = true;

        _processor!.SetSoftClipper(clipper);
        _processor.IsSoftClipperEnabled = false; // Start disabled

        float latency = _processor.SoftClipperLatencyMs;
        Console.WriteLine($"Soft Clipper: Configured ({latency:F1}ms latency, press C to enable)");
    }

    /// <summary>
    /// Configure LUFS Metering.
    /// </summary>
    static void ConfigureLufs()
    {
        // LUFS metering is enabled by default
        _processor!.IsLufsEnabled = true;
        _processor.ResetLufs(); // Start fresh

        Console.WriteLine("LUFS Metering: Enabled");
    }
}
