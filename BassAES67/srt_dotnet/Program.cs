// BASS SRT Plugin Test Application (C#)
// Usage: dotnet run [srt://host:port]

using System.Runtime.InteropServices;

// Set native library search path to include the application directory
NativeLibrary.SetDllImportResolver(typeof(BassSrtNative).Assembly, (libraryName, assembly, searchPath) =>
{
    // Get the directory where the executable is located
    string? assemblyDir = Path.GetDirectoryName(assembly.Location);
    if (assemblyDir == null)
        return IntPtr.Zero;

    // Try to load from the application directory first
    string fullPath = Path.Combine(assemblyDir, libraryName);

    // Add platform-specific extension if not present
    if (RuntimeInformation.IsOSPlatform(OSPlatform.Linux))
    {
        if (!libraryName.EndsWith(".so"))
            fullPath = Path.Combine(assemblyDir, $"lib{libraryName}.so");
    }
    else if (RuntimeInformation.IsOSPlatform(OSPlatform.Windows))
    {
        if (!libraryName.EndsWith(".dll"))
            fullPath = Path.Combine(assemblyDir, $"{libraryName}.dll");
    }

    if (File.Exists(fullPath) && NativeLibrary.TryLoad(fullPath, out IntPtr handle))
        return handle;

    // Fall back to default resolution
    return IntPtr.Zero;
});

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

// Load the SRT plugin - use absolute path from application directory
Console.WriteLine("\nLoading SRT plugin...");

string? appDir = Path.GetDirectoryName(System.Reflection.Assembly.GetExecutingAssembly().Location);
string[] pluginPaths = appDir != null
    ? new[]
    {
        Path.Combine(appDir, "libbass_srt.so"),
        "libbass_srt.so",
    }
    : new[]
    {
        "libbass_srt.so",
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

// Stream handle - will be updated on reconnection
int stream = 0;

// Reconnection timer (one-shot, triggered on disconnect)
System.Timers.Timer? reconnectTimer = null;

// Function to create stream and start playback
bool CreateStreamAndPlay()
{
    // Clean up old stream if exists
    if (stream != 0)
    {
        BassSrtNative.BASS_ChannelStop(stream);
        BassSrtNative.BASS_StreamFree(stream);
        stream = 0;
    }

    stream = BassSrtNative.BASS_StreamCreateURL(url, 0, 0, IntPtr.Zero, IntPtr.Zero);
    if (stream == 0)
    {
        Console.WriteLine($"\n[Reconnect] Failed to create stream (error: {BassSrtNative.BASS_ErrorGetCode()})");
        return false;
    }

    if (!BassSrtNative.BASS_ChannelPlay(stream, false))
    {
        Console.WriteLine($"\n[Reconnect] Failed to start playback (error: {BassSrtNative.BASS_ErrorGetCode()})");
        return false;
    }

    return true;
}

// Function to schedule reconnection attempt
void ScheduleReconnect(int delayMs = 3000)
{
    // Dispose old timer if exists
    reconnectTimer?.Dispose();

    reconnectTimer = new System.Timers.Timer(delayMs);
    reconnectTimer.AutoReset = false; // One-shot
    reconnectTimer.Elapsed += (s, e) =>
    {
        Console.WriteLine("\n[Reconnect] Attempting to reconnect...");
        if (CreateStreamAndPlay())
        {
            Console.WriteLine("[Reconnect] Reconnected successfully!");
        }
        else
        {
            // Failed - try again in 3 seconds
            ScheduleReconnect(3000);
        }
    };
    reconnectTimer.Start();
    Console.WriteLine($"[SRT] Will attempt reconnect in {delayMs / 1000} seconds...");
}

// Set up connection state callback (called from Rust when state changes)
// IMPORTANT: Keep a reference to prevent garbage collection!
BassSrtNative.ConnectionStateCallback connectionCallback = (state, user) =>
{
    string stateName = BassSrtNative.GetConnectionStateName((int)state);
    Console.WriteLine($"\n[SRT] Connection state changed: {stateName}");

    if (state == BassSrtNative.CONNECTION_STATE_DISCONNECTED)
    {
        Console.WriteLine("[SRT] Sender disconnected.");
        // Schedule reconnection attempt
        ScheduleReconnect(3000);
    }
    else if (state == BassSrtNative.CONNECTION_STATE_CONNECTED)
    {
        Console.WriteLine("[SRT] Connected to sender.");
    }
};
BassSrtNative.BASS_SRT_SetConnectionStateCallback(connectionCallback, IntPtr.Zero);

// Initial connection
Console.WriteLine($"\nConnecting to SRT stream: {url}");
Console.WriteLine("(Make sure an SRT sender is running on that address)\n");

if (!CreateStreamAndPlay())
{
    Console.WriteLine("\nTo test, start the SRT sender first in bass-srt folder:");
    Console.WriteLine("  ./run_sender.sh opus");
    // Don't exit - schedule reconnect attempt
    ScheduleReconnect(3000);
}
else
{
    Console.WriteLine("Playback started!");
}

Console.WriteLine("\nPress Ctrl+C to stop...\n");

// Set up exit event for clean shutdown
using var exitEvent = new ManualResetEventSlim(false);
Console.CancelKeyPress += (s, e) =>
{
    e.Cancel = true;
    exitEvent.Set();
};

// Set up status timer
using var statusTimer = new System.Timers.Timer(500);
statusTimer.Elapsed += (s, e) =>
{
    try
    {
        if (stream != 0)
        {
            UpdateStatus(stream);
        }
        else
        {
            Console.Write("\rWaiting for connection...                                                        ");
        }
    }
    catch (Exception ex)
    {
        Console.WriteLine($"\n[Timer Error] {ex.Message}");
    }
};
statusTimer.AutoReset = true;
statusTimer.Start();

// Wait for Ctrl+C
exitEvent.Wait();

// Cleanup
Console.WriteLine("\n\nCleaning up...");
statusTimer.Stop();
reconnectTimer?.Stop();
reconnectTimer?.Dispose();
BassSrtNative.BASS_SRT_ClearConnectionStateCallback();
if (stream != 0)
{
    BassSrtNative.BASS_ChannelStop(stream);
    BassSrtNative.BASS_StreamFree(stream);
}
BassSrtNative.BASS_PluginFree(plugin);
BassSrtNative.BASS_Free();
Console.WriteLine("Done!");

// Status update function
void UpdateStatus(int streamHandle)
{
    int state = BassSrtNative.BASS_ChannelIsActive(streamHandle);
    string stateStr = BassSrtNative.GetStateName(state);

    // Get audio level
    uint level = BassSrtNative.BASS_ChannelGetLevel(streamHandle);
    double left = BassSrtNative.GetLeftLevelPercent(level);
    double right = BassSrtNative.GetRightLevelPercent(level);

    // Get SRT stats
    uint codec = BassSrtNative.BASS_GetConfig(BassSrtNative.BASS_CONFIG_SRT_CODEC);
    uint bitrate = BassSrtNative.BASS_GetConfig(BassSrtNative.BASS_CONFIG_SRT_BITRATE);

    // Get SRT transport stats
    double rtt = BassSrtNative.GetRttMs();
    uint loss = BassSrtNative.BASS_GetConfig(BassSrtNative.BASS_CONFIG_SRT_LOSS_TOTAL);
    uint uptime = BassSrtNative.BASS_GetConfig(BassSrtNative.BASS_CONFIG_SRT_UPTIME);
    uint connState = BassSrtNative.BASS_GetConfig(BassSrtNative.BASS_CONFIG_SRT_CONNECTION_STATE);

    string codecStr = BassSrtNative.GetCodecName((int)codec);
    string bitrateStr = bitrate > 0 ? $"{bitrate}k" : "-";
    string connStr = BassSrtNative.GetConnectionStateName((int)connState);

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
    Console.Write($"\r{stateStr,-8} {connStr,-12} [{codecStr,4} {bitrateStr,4}] L[{leftMeter}] R[{rightMeter}] | RTT:{rtt:F1}ms Loss:{loss} Up:{uptimeMin}:{uptimeSec:D2}  ");
}
