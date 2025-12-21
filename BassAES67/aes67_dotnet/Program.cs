using System.Net;
using Un4seen.Bass;
using Un4seen.Bass.AddOn.Mix;

// Parse command line args
string clockMode = args.Length > 0 ? args[0].ToLower() : "sys"; // NOT USED!
string interfaceIp = args.Length > 1 ? args[1] : "192.168.60.104";
string inputMulticast = args.Length > 2 ? args[2] : "239.192.76.49";
string outputMulticast = args.Length > 3 ? args[3] : "239.192.1.100";

// Initialize BASS
var audioEngine = new AudioEngine();
audioEngine.InitBass(0);  // device=0 for no-soundcard mode

int mixer = BassMix.BASS_Mixer_StreamCreate(48000, 2, BASSFlag.BASS_STREAM_DECODE | BASSFlag.BASS_SAMPLE_SOFTWARE | BASSFlag.BASS_MIXER_NONSTOP);
Console.WriteLine($"mixer: {mixer}");


// Set clock mode BEFORE creating streams
int clockModeValue = Aes67Native.BASS_AES67_CLOCK_PTP;   // BASS_AES67_CLOCK_PTP, BASS_AES67_CLOCK_LIVEWIRE, BASS_AES67_CLOCK_SYSTEM
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

// Create input stream (decode mode)



string inputUrl = $"aes67://{inputMulticast}:5004";
Console.WriteLine($"Creating AES67 input stream... {inputUrl}");
Console.WriteLine("Using direct P/Invoke (bypassing Bass.NET)...");
int inputStream = Aes67Native.BASS_StreamCreateURL_Direct(inputUrl, 0, Aes67Native.BASS_STREAM_DECODE, IntPtr.Zero, IntPtr.Zero);
Console.WriteLine($"BASS_StreamCreateURL (aes67 plugin): {Bass.BASS_ErrorGetCode()}, handle={inputStream}");

//Add inputStream stream to mixer
BassMix.BASS_Mixer_StreamAddChannel(mixer, inputStream, BASSFlag.BASS_STREAM_AUTOFREE);
Console.WriteLine($"BASS_Mixer_StreamAddChannel: {Bass.BASS_ErrorGetCode()}");


// THIS CODE WORKS, it plays
/*
int testChannel = Bass.BASS_StreamCreateURL("https://live1.sr.se/p4malm-aac-128", 0, BASSFlag.BASS_STREAM_DECODE, null, IntPtr.Zero);

//Add testChannel stream to mixer
BassMix.BASS_Mixer_StreamAddChannel(mixer, testChannel, BASSFlag.BASS_STREAM_AUTOFREE);
Console.WriteLine($"BASS_Mixer_StreamAddChannel: {Bass.BASS_ErrorGetCode()}");
*/


// Wait for input buffer to fill (>50%)
Console.WriteLine("Waiting for input buffer...");
for (int i = 0; i < 50; i++)
{
    int bufferLevel = Bass.BASS_GetConfig((BASSConfig)Aes67Native.BASS_CONFIG_AES67_BUFFER_LEVEL);
    if (bufferLevel >= 50)
    {
        Console.WriteLine($"Buffer ready ({bufferLevel}%)");
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

Console.WriteLine($"Output stream created (dest: {outputMulticast}:5004, {outputConfig.PacketTimeUs/1000}ms/{outputConfig.PacketsPerSecond}pkt/s)\n");


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

    // Get clock stats (use direct function for detailed stats)
    int clockLocked = Bass.BASS_GetConfig((BASSConfig)Aes67Native.BASS_CONFIG_AES67_PTP_LOCKED);
    int clockState = Bass.BASS_GetConfig((BASSConfig)Aes67Native.BASS_CONFIG_AES67_PTP_STATE);
    string? clockStats = Aes67Native.GetClockStats();

    // Get output stats
    long outPackets = outputStream.PacketsSent;
    long outUnderruns = outputStream.Underruns;

    // Display stats - use detailed clock stats if available, otherwise fallback to simple status
    string lockStatus = clockStats ?? (clockLocked != 0 ? "LOCKED" : Aes67Native.GetClockStateName(clockState));
    Console.Write($"\rIN: {bufferLevel}/{targetPackets} rcv={packetsReceived} late={packetsLate} und={underruns} | ");
    Console.Write($"OUT: pkt={outPackets} und={outUnderruns} | ");
    Console.Write($"{lockStatus}   ");

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
//Bass.BASS_PluginFree(pluginHandle);
Bass.BASS_Free();
Console.WriteLine("Done.");
