// BASS RTP Plugin Test Application (C#)
// Tests the OUTPUT module - Z/IP ONE connects TO us
//
// Usage: dotnet run [port] [backfeed_codec]
//   port           - Local port to listen on (default: 6004)
//   backfeed_codec - 0=PCM16, 3=MP2, 4=G.711, 5=G.722 (default: 4)

using System.Runtime.InteropServices;
using Un4seen.Bass;
using Un4seen.Bass.AddOn.Mix;

Console.WriteLine("BASS RTP Plugin Test (C#)");
Console.WriteLine("=========================\n");

// Parse command line
ushort localPort = args.Length > 0 ? ushort.Parse(args[0]) : (ushort)6004;
byte backfeedCodec = args.Length > 1 ? byte.Parse(args[1]) : BassRtpNative.BASS_RTP_CODEC_G711;

Console.WriteLine($"Mode: OUTPUT (Z/IP ONE connects TO us)");
Console.WriteLine($"Listen port:    {localPort}");
Console.WriteLine($"Backfeed codec: {BassRtpNative.GetCodecName(backfeedCodec)}");
Console.WriteLine();

// Initialize BASS
Console.WriteLine("Initializing BASS...");
BassNet.Registration("kennet@kennet.se", "2X20231816202323");

if (!Bass.BASS_Init(-1, 48000, BASSInit.BASS_DEVICE_DEFAULT, IntPtr.Zero))
{
    Console.WriteLine($"ERROR: Failed to initialize BASS (error: {Bass.BASS_ErrorGetCode()})");
    return;
}

var version = Bass.BASS_GetVersion();
Console.WriteLine($"BASS version: {(version >> 24) & 0xFF}.{(version >> 16) & 0xFF}.{(version >> 8) & 0xFF}.{version & 0xFF}");

// Load the RTP plugin
Console.WriteLine("\nLoading RTP plugin...");
bool isLinux = RuntimeInformation.IsOSPlatform(OSPlatform.Linux);
string pluginName = isLinux ? "libbass_rtp.so" : "bass_rtp.dll";
int pluginHandle = Bass.BASS_PluginLoad(pluginName);
if (pluginHandle == 0)
{
    // Try alternate paths
    string[] paths = isLinux
        ? ["./libbass_rtp.so", "../bass-rtp/target/release/libbass_rtp.so"]
        : ["./bass_rtp.dll", "../bass-rtp/target/release/bass_rtp.dll"];

    foreach (var path in paths)
    {
        pluginHandle = Bass.BASS_PluginLoad(path);
        if (pluginHandle != 0)
        {
            Console.WriteLine($"Loaded plugin from: {path}");
            break;
        }
    }
}

if (pluginHandle == 0)
{
    Console.WriteLine($"WARNING: Plugin not loaded as BASS plugin (error: {Bass.BASS_ErrorGetCode()})");
    Console.WriteLine("         Will try direct DLL import...");
}
else
{
    Console.WriteLine($"Plugin loaded (handle: {pluginHandle})");
}

// Create a mixer for backfeed audio (NONSTOP outputs silence when nothing connected)
Console.WriteLine("\nCreating mixer for backfeed...");
int mixer = BassMix.BASS_Mixer_StreamCreate(48000, 2,
    BASSFlag.BASS_STREAM_DECODE | BASSFlag.BASS_SAMPLE_SOFTWARE | BASSFlag.BASS_MIXER_NONSTOP);

if (mixer == 0)
{
    Console.WriteLine($"ERROR: Failed to create mixer (error: {Bass.BASS_ErrorGetCode()})");
    Bass.BASS_Free();
    return;
}
Console.WriteLine("Mixer created (sends silence for backfeed)");

// Create RTP Output configuration
var config = BassRtpNative.RtpOutputConfigFFI.CreateDefault(localPort);
config.BackfeedCodec = backfeedCodec;
config.BackfeedBitrate = 256;
config.BufferMs = 200;

// Set up connection callback
BassRtpNative.ConnectionStateCallback? connectionCallback = (state, user) =>
{
    string stateName = BassRtpNative.GetConnectionStateName(state);
    Console.WriteLine($"\n>>> {stateName.ToUpper()} - RTP stream {(state == 1 ? "established" : "lost")}");
};

// Keep callback reference alive
GC.KeepAlive(connectionCallback);

// Note: Connection callback in struct requires special handling - skip for now

Console.WriteLine("\nCreating RTP Output stream...");
IntPtr rtpHandle = BassRtpNative.BASS_RTP_OutputCreate(mixer, ref config);

if (rtpHandle == IntPtr.Zero)
{
    Console.WriteLine($"ERROR: Failed to create RTP Output (error: {BassRtpNative.BASS_ErrorGetCode()})");
    Bass.BASS_StreamFree(mixer);
    Bass.BASS_Free();
    return;
}
Console.WriteLine("RTP Output stream created");

// Start the RTP Output stream
Console.WriteLine("Starting RTP Output stream...");
if (BassRtpNative.BASS_RTP_OutputStart(rtpHandle) == 0)
{
    Console.WriteLine($"ERROR: Failed to start RTP Output (error: {BassRtpNative.BASS_ErrorGetCode()})");
    BassRtpNative.BASS_RTP_OutputFree(rtpHandle);
    Bass.BASS_StreamFree(mixer);
    Bass.BASS_Free();
    return;
}
Console.WriteLine("RTP Output stream started - listening for connections");

// Get the incoming stream handle
int inputStream = BassRtpNative.BASS_RTP_OutputGetInputStream(rtpHandle);
if (inputStream == 0)
{
    Console.WriteLine("Incoming stream not available yet (will receive when data arrives)");
}
else
{
    Console.WriteLine($"Incoming stream ready (handle: {inputStream})");
    Bass.BASS_ChannelPlay(inputStream, false);
}

Console.WriteLine($"\n--- Running (Ctrl+C to stop) ---\n");
Console.WriteLine($"Waiting for Z/IP ONE to connect on port {localPort}...\n");

// Set up exit event for clean shutdown
using var exitEvent = new ManualResetEventSlim(false);
Console.CancelKeyPress += (s, e) =>
{
    e.Cancel = true;
    exitEvent.Set();
};

// Status monitoring loop
var startTime = DateTime.Now;
ulong lastRx = 0, lastTx = 0;
bool wasConnected = false;

using var statusTimer = new System.Timers.Timer(500);
statusTimer.Elapsed += (s, e) =>
{
    try
    {
        // Get statistics
        if (BassRtpNative.BASS_RTP_OutputGetStats(rtpHandle, out var stats) == 0)
            return;

        // Check if we just got connected
        bool isConnected = stats.RxPackets > 0 && stats.BufferLevel > 0;
        if (isConnected && !wasConnected)
        {
            Console.WriteLine("\n>>> CONNECTED - Receiving RTP stream");

            // Start playback now that we have data
            inputStream = BassRtpNative.BASS_RTP_OutputGetInputStream(rtpHandle);
            if (inputStream != 0)
            {
                Bass.BASS_ChannelPlay(inputStream, false);
            }
        }
        else if (!isConnected && wasConnected && stats.RxPackets > lastRx)
        {
            // Still receiving but buffer empty - might be reconnecting
        }
        wasConnected = isConnected;

        // Calculate packets per second
        ulong rxPps = (stats.RxPackets - lastRx) * 2;
        ulong txPps = (stats.TxPackets - lastTx) * 2;
        lastRx = stats.RxPackets;
        lastTx = stats.TxPackets;

        // Get channel level
        float leftLevel = 0, rightLevel = 0;
        if (inputStream != 0)
        {
            int level = Bass.BASS_ChannelGetLevel(inputStream);
            leftLevel = (level & 0xFFFF) / 327.68f;
            rightLevel = ((level >> 16) & 0xFFFF) / 327.68f;
        }

        // Get channel state
        var state = inputStream != 0 ? Bass.BASS_ChannelIsActive(inputStream) : BASSActive.BASS_ACTIVE_STOPPED;
        string stateStr = state switch
        {
            BASSActive.BASS_ACTIVE_PLAYING => "Play",
            BASSActive.BASS_ACTIVE_STALLED => "Stal",
            BASSActive.BASS_ACTIVE_PAUSED => "Paus",
            _ => "Stop"
        };

        // Format elapsed time
        var elapsed = DateTime.Now - startTime;
        int mins = (int)elapsed.TotalMinutes;
        int secs = elapsed.Seconds;

        // Create level meter
        int meterWidth = 10;
        int leftBars = Math.Min((int)(leftLevel * meterWidth / 100), meterWidth);
        int rightBars = Math.Min((int)(rightLevel * meterWidth / 100), meterWidth);
        string leftMeter = new string('|', leftBars) + new string(' ', meterWidth - leftBars);
        string rightMeter = new string('|', rightBars) + new string(' ', meterWidth - rightBars);

        // Connection state
        string connStr = isConnected ? "CONN" : "----";

        // PPM
        double ppm = BassRtpNative.GetPpm(stats.CurrentPpmX1000);

        // Detected codec
        string codecStr = BassRtpNative.GetPayloadTypeName(stats.DetectedIncomingPt);

        // Print status line
        Console.Write($"\r[{mins:D2}:{secs:D2}] {connStr} {stateStr} RX:{stats.RxPackets,6}({rxPps,3}pps) TX:{stats.TxPackets,6}({txPps,4}pps) Buf:{stats.BufferLevel,5} Drop:{stats.RxDropped} [{leftMeter}][{rightMeter}] In:{codecStr} PPM:{ppm:+0.0;-0.0}");
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
Console.WriteLine("\n\nStopping...");
statusTimer.Stop();
BassRtpNative.BASS_RTP_OutputStop(rtpHandle);
BassRtpNative.BASS_RTP_OutputFree(rtpHandle);
Bass.BASS_StreamFree(mixer);
if (pluginHandle != 0) Bass.BASS_PluginFree(pluginHandle);
Bass.BASS_Free();
Console.WriteLine("Done!");
