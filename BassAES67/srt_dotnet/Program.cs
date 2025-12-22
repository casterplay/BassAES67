// BASS SRT Plugin Test Application (C#)
// Usage: dotnet run [srt://host:port]

Console.WriteLine("BASS SRT Plugin Test (C#)");
Console.WriteLine("=========================\n");

// Parse command line
string url = args.Length > 0 ? args[0] : "srt://127.0.0.1:9000";

// Get BASS version
Console.WriteLine($"BASS version: {BassSrtNative.GetVersionString()}");

// Initialize BASS
Console.WriteLine("\nInitializing BASS...");
if (!BassSrtNative.BASS_Init(-1, 48000, 0, IntPtr.Zero, IntPtr.Zero))
{
    Console.WriteLine($"ERROR: Failed to initialize BASS (error code: {BassSrtNative.BASS_ErrorGetCode()})");
    return;
}
Console.WriteLine("BASS initialized successfully");

// Load the SRT plugin
Console.WriteLine("\nLoading SRT plugin...");

string[] pluginPaths = new[]
{
    "libbass_srt.so",
    "./libbass_srt.so",
    "../bass-srt/target/release/libbass_srt.so",
};

int plugin = 0;
foreach (var path in pluginPaths)
{
    plugin = BassSrtNative.BASS_PluginLoad(path, 0);
    if (plugin != 0)
    {
        Console.WriteLine($"Plugin loaded from: {path}");
        break;
    }
}

if (plugin == 0)
{
    Console.WriteLine($"ERROR: Failed to load plugin (error code: {BassSrtNative.BASS_ErrorGetCode()})");
    Console.WriteLine("Tried paths: " + string.Join(", ", pluginPaths));
    BassSrtNative.BASS_Free();
    return;
}
Console.WriteLine($"Plugin loaded successfully (handle: {plugin})");

// Create stream from SRT URL
Console.WriteLine($"\nConnecting to SRT stream: {url}");
Console.WriteLine("(Make sure an SRT sender is running on that address)\n");

int stream = BassSrtNative.BASS_StreamCreateURL(url, 0, 0, IntPtr.Zero, IntPtr.Zero);

if (stream == 0)
{
    int error = BassSrtNative.BASS_ErrorGetCode();
    Console.WriteLine($"ERROR: Failed to create stream (error code: {error})");
    Console.WriteLine("\nTo test, start the SRT sender first:");
    Console.WriteLine("  ./run_sender.sh opus");
    BassSrtNative.BASS_PluginFree(plugin);
    BassSrtNative.BASS_Free();
    return;
}
Console.WriteLine($"Stream created (handle: {stream})");

// Start playback
Console.WriteLine("\nStarting playback...");
if (!BassSrtNative.BASS_ChannelPlay(stream, false))
{
    Console.WriteLine($"ERROR: Failed to start playback (error code: {BassSrtNative.BASS_ErrorGetCode()})");
    BassSrtNative.BASS_StreamFree(stream);
    BassSrtNative.BASS_PluginFree(plugin);
    BassSrtNative.BASS_Free();
    return;
}
Console.WriteLine("Playback started!");
Console.WriteLine("\nPress Ctrl+C to stop...\n");

// Handle Ctrl+C
bool running = true;
Console.CancelKeyPress += (s, e) =>
{
    e.Cancel = true;
    running = false;
};

// Monitor playback
while (running)
{
    int state = BassSrtNative.BASS_ChannelIsActive(stream);
    string stateStr = BassSrtNative.GetStateName(state);

    // Get audio level
    uint level = BassSrtNative.BASS_ChannelGetLevel(stream);
    double left = BassSrtNative.GetLeftLevelPercent(level);
    double right = BassSrtNative.GetRightLevelPercent(level);

    // Get SRT stats
    uint bufferLevel = BassSrtNative.BASS_GetConfig(BassSrtNative.BASS_CONFIG_SRT_BUFFER_LEVEL);
    uint codec = BassSrtNative.BASS_GetConfig(BassSrtNative.BASS_CONFIG_SRT_CODEC);
    uint bitrate = BassSrtNative.BASS_GetConfig(BassSrtNative.BASS_CONFIG_SRT_BITRATE);

    // Get SRT transport stats
    double rtt = BassSrtNative.GetRttMs();
    uint bandwidth = BassSrtNative.BASS_GetConfig(BassSrtNative.BASS_CONFIG_SRT_BANDWIDTH);
    uint loss = BassSrtNative.BASS_GetConfig(BassSrtNative.BASS_CONFIG_SRT_LOSS_TOTAL);
    uint retrans = BassSrtNative.BASS_GetConfig(BassSrtNative.BASS_CONFIG_SRT_RETRANS_TOTAL);
    uint uptime = BassSrtNative.BASS_GetConfig(BassSrtNative.BASS_CONFIG_SRT_UPTIME);

    string codecStr = BassSrtNative.GetCodecName((int)codec);
    string bitrateStr = bitrate > 0 ? $"{bitrate}k" : "-";

    // Create level meter
    int meterWidth = 10;
    int leftBars = Math.Min((int)(left * meterWidth / 100), meterWidth);
    int rightBars = Math.Min((int)(right * meterWidth / 100), meterWidth);
    string leftMeter = new string('#', leftBars).PadRight(meterWidth);
    string rightMeter = new string('#', rightBars).PadRight(meterWidth);

    // Format uptime
    uint uptimeMin = uptime / 60;
    uint uptimeSec = uptime % 60;

    // Print status
    Console.Write($"\r{stateStr,-8} [{codecStr,4} {bitrateStr,4}] L[{leftMeter}] R[{rightMeter}] | RTT:{rtt:F1}ms BW:{bandwidth}k Loss:{loss} Up:{uptimeMin}:{uptimeSec:D2}  ");

    if (state == BassSrtNative.BASS_ACTIVE_STOPPED)
    {
        Console.WriteLine("\n\nStream ended");
        break;
    }

    Thread.Sleep(500);
}

// Cleanup
Console.WriteLine("\nCleaning up...");
BassSrtNative.BASS_ChannelStop(stream);
BassSrtNative.BASS_StreamFree(stream);
BassSrtNative.BASS_PluginFree(plugin);
BassSrtNative.BASS_Free();
Console.WriteLine("Done!");
