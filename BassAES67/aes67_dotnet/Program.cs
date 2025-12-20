using System.Net;
using Un4seen.Bass;
using Un4seen.Bass.AddOn.Mix;

/// <summary>
/// AES67 Loopback Example
/// Receives AES67 audio, routes through BASS, transmits to different multicast
/// </summary>


// Parse command line args
string clockMode = args.Length > 0 ? args[0].ToLower() : "sys";
string interfaceIp = args.Length > 1 ? args[1] : "192.168.60.102";
string inputMulticast = args.Length > 2 ? args[2] : "239.192.76.49";
string outputMulticast = args.Length > 3 ? args[3] : "239.192.1.100";

/*int clockModeValue = clockMode switch
{
    "ptp" => Aes67Native.BASS_AES67_CLOCK_PTP,
    "lw" => Aes67Native.BASS_AES67_CLOCK_LIVEWIRE,
    _ => Aes67Native.BASS_AES67_CLOCK_SYSTEM
};
*/

int clockModeValue = Aes67Native.BASS_AES67_CLOCK_PTP;
/*
Console.WriteLine($"Clock Mode: {Aes67Native.GetClockModeName(clockModeValue)}");
Console.WriteLine($"Interface:  {interfaceIp}");
Console.WriteLine($"Input:      {inputMulticast}:5004");
Console.WriteLine($"Output:     {outputMulticast}:5004\n");
*/
// Initialize BASS
var audioEngine = new AudioEngine();
audioEngine.InitBass(0);  // device=0 for no-soundcard mode

int mixer = BassMix.BASS_Mixer_StreamCreate(48000, 2, BASSFlag.BASS_STREAM_DECODE | BASSFlag.BASS_SAMPLE_SOFTWARE | BASSFlag.BASS_MIXER_NONSTOP);
Console.WriteLine($"mixer: {mixer}");

// Load AES67 plugin
int pluginHandle = Bass.BASS_PluginLoad("bass_aes67.dll");
if (pluginHandle == 0)
{
    Console.WriteLine($"ERROR - Failed to load bass_aes67.dll: {Bass.BASS_ErrorGetCode()}");
    return;
}
Console.WriteLine("  bass_aes67.dll loaded");

// Set clock mode BEFORE creating streams
Bass.BASS_SetConfig((BASSConfig)Aes67Native.BASS_CONFIG_AES67_CLOCK_MODE, clockModeValue);
Console.WriteLine($"  Clock mode set to: {Aes67Native.GetClockModeName(clockModeValue)} ({clockModeValue})");

// Configure AES67
int ptpDomain = 1; 
Aes67Native.BASS_SetConfigPtr(Aes67Native.BASS_CONFIG_AES67_INTERFACE, interfaceIp);
Bass.BASS_SetConfig((BASSConfig)Aes67Native.BASS_CONFIG_AES67_JITTER, 10);  // 10ms jitter buffer
Bass.BASS_SetConfig((BASSConfig)Aes67Native.BASS_CONFIG_AES67_PTP_DOMAIN, ptpDomain); 
Console.WriteLine($"  AES67 configured (interface={interfaceIp}, jitter=10ms, domain={ptpDomain})\n");

// Start clock WITHOUT needing an AES67 input stream!
Aes67Native.BASS_AES67_ClockStart();

// Create input stream (decode mode)

/*
Console.WriteLine("Creating AES67 input stream...");
string inputUrl = $"aes67://{inputMulticast}:5004";
int inputStream = Bass.BASS_StreamCreateURL(inputUrl, 0, BASSFlag.BASS_STREAM_DECODE, null, IntPtr.Zero);
if (inputStream == 0)
{
    Console.WriteLine($"ERROR - Failed to create input stream: {Bass.BASS_ErrorGetCode()}");
    return;
}
Console.WriteLine($"  Input stream created (source: {inputMulticast}:5004)\n");

//Add inputStream stream to mixer
BassMix.BASS_Mixer_StreamAddChannel(mixer, inputStream, BASSFlag.BASS_STREAM_AUTOFREE);
Console.WriteLine($"BASS_Mixer_StreamAddChannel: {Bass.BASS_ErrorGetCode()}");
*/

string p3= "https://live1.sr.se/p3-aac-320";

int testChannel = Bass.BASS_StreamCreateURL("https://live1.sr.se/p4malm-aac-128", 0, BASSFlag.BASS_STREAM_DECODE, null, IntPtr.Zero);

//Add testChannel stream to mixer
BassMix.BASS_Mixer_StreamAddChannel(mixer, testChannel, BASSFlag.BASS_STREAM_AUTOFREE);
Console.WriteLine($"BASS_Mixer_StreamAddChannel: {Bass.BASS_ErrorGetCode()}");


// Wait for clock lock
Console.WriteLine($"Waiting for {Aes67Native.GetClockModeName(clockModeValue)} lock...");
int lockWaitSeconds = 10;
for (int i = 0; i < lockWaitSeconds * 10; i++)
{
    int locked = Bass.BASS_GetConfig((BASSConfig)Aes67Native.BASS_CONFIG_AES67_PTP_LOCKED);
    int state = Bass.BASS_GetConfig((BASSConfig)Aes67Native.BASS_CONFIG_AES67_PTP_STATE);

    if (locked != 0)
    {
        Console.WriteLine($"  {Aes67Native.GetClockModeName(clockModeValue)} locked!");
        break;
    }

    Console.Write($"\r  State: {Aes67Native.GetClockStateName(state)}... ");
    Thread.Sleep(100);

    if (i == lockWaitSeconds * 10 - 1)
    {
        Console.WriteLine("\n  WARNING: Clock not locked after timeout, continuing anyway...");
    }
}
Console.WriteLine();

// Wait for input buffer to fill (>50%)
Console.WriteLine("Waiting for input buffer...");
for (int i = 0; i < 50; i++)
{
    int bufferLevel = Bass.BASS_GetConfig((BASSConfig)Aes67Native.BASS_CONFIG_AES67_BUFFER_LEVEL);
    if (bufferLevel >= 50)
    {
        Console.WriteLine($"  Buffer ready ({bufferLevel}%)");
        break;
    }
    Thread.Sleep(100);
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

Console.WriteLine($"  Output stream created (dest: {outputMulticast}:5004, {outputConfig.PacketTimeUs/1000}ms/{outputConfig.PacketsPerSecond}pkt/s)\n");

// Print status header
Console.WriteLine("==========================================");
Console.WriteLine($"Loopback running ({Aes67Native.GetClockModeName(clockModeValue)} sync):");
Console.WriteLine($"  INPUT:  {inputMulticast}:5004");
Console.WriteLine($"  OUTPUT: {outputMulticast}:5004");
Console.WriteLine("==========================================");
Console.WriteLine("Press Ctrl+C to stop\n");

// Setup Ctrl+C handler
var cts = new CancellationTokenSource();
Console.CancelKeyPress += (s, e) =>
{
    e.Cancel = true;
    cts.Cancel();
};

// Monitor loop
while (!cts.Token.IsCancellationRequested)
{
    // Get input stats
    int bufferLevel = Bass.BASS_GetConfig((BASSConfig)Aes67Native.BASS_CONFIG_AES67_BUFFER_LEVEL);
    int targetPackets = Bass.BASS_GetConfig((BASSConfig)Aes67Native.BASS_CONFIG_AES67_TARGET_PACKETS);
    int packetsReceived = Bass.BASS_GetConfig((BASSConfig)Aes67Native.BASS_CONFIG_AES67_PACKETS_RECEIVED);
    int packetsLate = Bass.BASS_GetConfig((BASSConfig)Aes67Native.BASS_CONFIG_AES67_PACKETS_LATE);
    int underruns = Bass.BASS_GetConfig((BASSConfig)Aes67Native.BASS_CONFIG_AES67_JITTER_UNDERRUNS);

    // Get clock stats
    int clockLocked = Bass.BASS_GetConfig((BASSConfig)Aes67Native.BASS_CONFIG_AES67_PTP_LOCKED);
    int clockState = Bass.BASS_GetConfig((BASSConfig)Aes67Native.BASS_CONFIG_AES67_PTP_STATE);
    string? clockStats = Aes67Native.GetConfigString(Aes67Native.BASS_CONFIG_AES67_PTP_STATS);

    // Get output stats
    long outPackets = outputStream.PacketsSent;
    long outUnderruns = outputStream.Underruns;

    // Display stats
    string lockStatus = clockLocked != 0 ? "LOCKED" : Aes67Native.GetClockStateName(clockState);
    Console.Write($"\rIN: {bufferLevel}/{targetPackets} rcv={packetsReceived} late={packetsLate} und={underruns} | ");
    Console.Write($"OUT: pkt={outPackets} und={outUnderruns} | ");
    Console.Write($"{Aes67Native.GetClockModeName(clockModeValue)} {lockStatus}   ");

    try
    {
        await Task.Delay(1000, cts.Token);
    }
    catch (TaskCanceledException)
    {
        break;
    }
}

// Cleanup
Console.WriteLine("\n\nShutting down...");
outputStream.Stop();
Aes67Native.BASS_AES67_ClockStop();
//Bass.BASS_StreamFree(inputStream);
Bass.BASS_PluginFree(pluginHandle);
Bass.BASS_Free();
Console.WriteLine("Done.");
