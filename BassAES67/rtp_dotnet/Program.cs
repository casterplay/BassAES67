// BASS RTP Plugin Test Application (C#)
// Tests the OUTPUT module - Z/IP ONE connects TO us
//
// Usage: dotnet run [port] [backfeed_codec]
//   port           - Local port to listen on (default: 6004)
//   backfeed_codec - 0=PCM16, 3=MP2, 4=G.711, 5=G.722 (default: 4)

using System.Runtime.InteropServices;
using Un4seen.Bass;
using Un4seen.Bass.AddOn.Mix;

using System.Net;

Console.WriteLine("BASS RTP Plugin Test (C#)");
Console.WriteLine("=========================\n");

// Parse command line
ushort localPort = args.Length > 0 ? ushort.Parse(args[0]) : (ushort)6004;
byte backfeedCodec = args.Length > 1 ? byte.Parse(args[1]) : BassRtpNative.BASS_RTP_CODEC_PCM16;

string interfaceIp = args.Length > 2 ? args[2] : "192.168.60.102";
string inputMulticast = args.Length > 3 ? args[3] : "239.192.76.50";
string outputMulticast = args.Length > 4 ? args[4] : "239.192.1.100";

IntPtr rtpHandle = IntPtr.Zero;

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



//GET AoIP Input stream (USED AS INPUT IN BASS-RTP)
string inputUrl = $"aes67://{inputMulticast}:5004";
Console.WriteLine($"Creating AES67 input stream... {inputUrl}");
Console.WriteLine("Using direct P/Invoke (bypassing Bass.NET)...");
int inputStream = Aes67Native.BASS_StreamCreateURL_Direct(inputUrl, 0, Aes67Native.BASS_STREAM_DECODE, IntPtr.Zero, IntPtr.Zero);
Console.WriteLine($"BASS_StreamCreateURL (aes67 plugin): {Bass.BASS_ErrorGetCode()}, handle={inputStream}");



// Create RTP Output configuration
var config = BassRtpNative.RtpOutputConfigFFI.CreateDefault(localPort);
config.BackfeedCodec = backfeedCodec;
config.BackfeedBitrate = 256;
config.BufferMs = 200;
config.DecodeStream = 1;  // Enable BASS_STREAM_DECODE for mixer compatibility


// Set up connection callback - store as variable that lives for program duration
BassRtpNative.ConnectionStateCallback connectionCallback = (state, user) =>
{
    string stateName = BassRtpNative.GetConnectionStateName(state);
    Console.WriteLine($"\n>>> {stateName.ToUpper()} - RTP stream {(state == 1 ? "established" : "lost")}");
};

// Pass callback to config via Marshal
config.ConnectionCallback = Marshal.GetFunctionPointerForDelegate(connectionCallback);

Console.WriteLine("\nCreating RTP Output stream...");
rtpHandle = BassRtpNative.BASS_RTP_OutputCreate(inputStream, ref config);

if (rtpHandle == IntPtr.Zero)
{
    Console.WriteLine($"ERROR: Failed to create RTP Output (error: {BassRtpNative.BASS_ErrorGetCode()})");
    return;
}
Console.WriteLine("RTP Output stream created");

// Start the RTP Output stream
Console.WriteLine("Starting RTP Output stream...");
if (BassRtpNative.BASS_RTP_OutputStart(rtpHandle) == 0)
{
    Console.WriteLine($"ERROR: Failed to start RTP Output (error: {BassRtpNative.BASS_ErrorGetCode()})");
    BassRtpNative.BASS_RTP_OutputFree(rtpHandle);
    return;
}
Console.WriteLine("RTP Output stream started - listening for connections");

// Get the incoming stream handle

int bassReturnChan = BassRtpNative.BASS_RTP_OutputGetInputStream(rtpHandle);
if (bassReturnChan == 0)
{
    Console.WriteLine("RTP Incoming stream not available yet (will receive when data arrives)");
}
else
{
    Console.WriteLine($"RTP Incoming stream ready (handle: {bassReturnChan})");

    BassMix.BASS_Mixer_StreamAddChannel(mixer, bassReturnChan, BASSFlag.BASS_STREAM_AUTOFREE);
    Console.WriteLine($"BASS_Mixer_StreamAddChannel: {Bass.BASS_ErrorGetCode()}");

}







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




// Cleanup
/*
Console.WriteLine("\n\nStopping...");
statusTimer.Stop();
BassRtpNative.BASS_RTP_OutputStop(rtpHandle);
BassRtpNative.BASS_RTP_OutputFree(rtpHandle);
Bass.BASS_StreamFree(mixer);
if (pluginHandle != 0) Bass.BASS_PluginFree(pluginHandle);
Bass.BASS_Free();
Console.WriteLine("Done!");
*/
Console.ReadLine();

// Keep callback alive until program ends (prevents GC from collecting it)
GC.KeepAlive(connectionCallback);
