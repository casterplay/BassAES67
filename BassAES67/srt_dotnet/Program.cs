// BASS SRT Plugin Test Application (C#)
// Usage: dotnet run [srt://host:port]

using System.Net;
using Un4seen.Bass;
using Un4seen.Bass.AddOn.Mix;
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

    Console.WriteLine($"Trying to load native library from: {fullPath}");

    if (File.Exists(fullPath) && NativeLibrary.TryLoad(fullPath, out IntPtr handle))
        return handle;

    // Fall back to default resolution
    return IntPtr.Zero;
});

Console.WriteLine("BASS SRT Plugin Test (C#)");
Console.WriteLine("=========================\n");

// Parse command line
string url = args.Length > 0 ? args[0] : "srt://127.0.0.1:9000";
string interfaceIp = args.Length > 1 ? args[1] : "192.168.60.102";
string inputMulticast = args.Length > 2 ? args[2] : "239.192.76.49";
string outputMulticast = args.Length > 3 ? args[3] : "239.192.1.100";


// Initialize BASS
var audioEngine = new AudioEngine();
audioEngine.InitBass(0);  // device=0 for no-soundcard mode

int mixer = BassMix.BASS_Mixer_StreamCreate(48000, 2, BASSFlag.BASS_STREAM_DECODE | BASSFlag.BASS_SAMPLE_SOFTWARE | BASSFlag.BASS_MIXER_NONSTOP);
Console.WriteLine($"mixer: {mixer}");


// Set clock mode BEFORE creating streams
int clockModeValue = Aes67Native.BASS_AES67_CLOCK_SYSTEM;   // BASS_AES67_CLOCK_PTP, BASS_AES67_CLOCK_LIVEWIRE, BASS_AES67_CLOCK_SYSTEM
Bass.BASS_SetConfig((BASSConfig)Aes67Native.BASS_CONFIG_AES67_CLOCK_MODE, clockModeValue);
Console.WriteLine($"Clock mode set to: {Aes67Native.GetClockModeName(clockModeValue)}");

// Configure AES67
int ptpDomain = 1; 
Aes67Native.BASS_SetConfigPtr(Aes67Native.BASS_CONFIG_AES67_INTERFACE, interfaceIp);
Bass.BASS_SetConfig((BASSConfig)Aes67Native.BASS_CONFIG_AES67_JITTER, 10);  // 10ms jitter buffer
Bass.BASS_SetConfig((BASSConfig)Aes67Native.BASS_CONFIG_AES67_PTP_DOMAIN, ptpDomain); 
Console.WriteLine($"AES67 configured (interface={interfaceIp}, jitter=10ms, domain={ptpDomain})\n");

// Start clock WITHOUT needing an AES67 input stream!
Aes67Native.BASS_AES67_ClockStart();

// Create output stream
    Console.WriteLine("Creating AES67 output stream...");
    var outputConfig = new Aes67OutputConfig
    {
        MulticastAddr = IPAddress.Parse(outputMulticast),
        Port = 5004,
        Interface = IPAddress.Parse(interfaceIp),
        Channels = 2,
        SampleRate = 48000,
        PacketTimeUs = 5000  // 5ms for Livewire compatibility
    };

    using var outputStream = new Aes67OutputStream(outputConfig);
    //outputStream.Start(inputStream);
    outputStream.Start(mixer);

Console.WriteLine($"Output stream created (dest: {outputMulticast}:5004, {outputConfig.PacketTimeUs/1000}ms/{outputConfig.PacketsPerSecond}pkt/s)\n");




// Wait for clock lock
Console.WriteLine($"Waiting for {Aes67Native.GetClockModeName(clockModeValue)} lock...");
int lockWaitSeconds = 10;
for (int i = 0; i < lockWaitSeconds * 10; i++)
{
    int locked = Bass.BASS_GetConfig((BASSConfig)Aes67Native.BASS_CONFIG_AES67_PTP_LOCKED);
    int state = Bass.BASS_GetConfig((BASSConfig)Aes67Native.BASS_CONFIG_AES67_PTP_STATE);

    if (locked != 0)
    {
        Console.WriteLine($"{Aes67Native.GetClockModeName(clockModeValue)} locked!");
        break;
    }

    Console.Write($"\rState: {Aes67Native.GetClockStateName(state)}... ");
    Thread.Sleep(100);

    if (i == lockWaitSeconds * 10 - 1)
    {
        Console.WriteLine("\nWARNING: Clock not locked after timeout, continuing anyway...");
    }
}
Console.WriteLine();


// Load the SRT plugin - use absolute path from application directory
Console.WriteLine("\nLoading SRT plugin...");

bool isLinux = RuntimeInformation.IsOSPlatform(OSPlatform.Linux);

int _pluginSrt = 0;
if(isLinux)
{
    _pluginSrt = Bass.BASS_PluginLoad("libbass_srt.so");
}
else
{
    _pluginSrt = Bass.BASS_PluginLoad("bass_srt.dll");
}


Console.WriteLine($"Plugin loaded (handle: {_pluginSrt} {Bass.BASS_ErrorGetCode()})");
/*

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
*/

// Stream handle - will be updated on reconnection
int stream = 0;

// Reconnection timer (one-shot, triggered on disconnect)
System.Timers.Timer? reconnectTimer = null;

// Function to create stream and start playback
bool CreateStreamAndPlay()
{
    
    //stream = BassSrtNative.BASS_StreamCreateURL(url, 0, 0, IntPtr.Zero, IntPtr.Zero);
    stream = BassSrtNative.BASS_StreamCreateURL(url, 0, Aes67Native.BASS_STREAM_DECODE, IntPtr.Zero, IntPtr.Zero);
    Console.WriteLine($"BASS_StreamCreateURL: {Bass.BASS_ErrorGetCode()}");
/*
    if (!BassSrtNative.BASS_ChannelPlay(stream, false))
    {
        Console.WriteLine($"\n[Reconnect] Failed to start playback (error: {BassSrtNative.BASS_ErrorGetCode()})");
        return false;
    }
*/

    //Add inputStream stream to mixer
    BassMix.BASS_Mixer_StreamAddChannel(mixer, stream, BASSFlag.BASS_STREAM_AUTOFREE);
    Console.WriteLine($"BASS_Mixer_StreamAddChannel: {Bass.BASS_ErrorGetCode()}");

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
        //ScheduleReconnect(3000);
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
    //ScheduleReconnect(3000);
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
BassSrtNative.BASS_PluginFree(_pluginSrt);
BassSrtNative.BASS_Free();
Console.WriteLine("Done!");

// Status update function
void UpdateStatus(int streamHandle)
{
    var state = Bass.BASS_ChannelIsActive(streamHandle);
    //string stateStr = BassSrtNative.GetStateName(state);

  

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



    // Format uptime
    uint uptimeMin = uptime / 60;
    uint uptimeSec = uptime % 60;

    // Print status
    Console.Write($"\r{state,-8} {connStr,-12} [{codecStr,4} {bitrateStr,4}] | RTT:{rtt:F1}ms Loss:{loss} Up:{uptimeMin}:{uptimeSec:D2}  ");
}
