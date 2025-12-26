using System.Runtime.InteropServices;
using Un4seen.Bass;
using Un4seen.Bass.AddOn.Mix;

using System.Net;
using BassWebRtc;
using System.Runtime.CompilerServices;

Console.WriteLine("BASS WebRTC Plugin Test (C#)");
Console.WriteLine("=========================\n");

BassWebRtcSignalingServer sigServer;
BassWebRtcPeer peer;

string interfaceIp =  "192.168.60.102";
string inputMulticast =  "239.192.76.50";
string outputMulticast = "239.192.1.100";

sigServer = new BassWebRtcSignalingServer(8080);
sigServer.Start();

// Initialize BASS
var audioEngine = new AudioEngine();
audioEngine.InitBass(0);  // device=0 for no-soundcard mode


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


/* AUDIO */


int mixer = BassMix.BASS_Mixer_StreamCreate(48000, 2, BASSFlag.BASS_STREAM_DECODE | BASSFlag.BASS_SAMPLE_SOFTWARE | BASSFlag.BASS_MIXER_NONSTOP);
Console.WriteLine($"mixer: {mixer}");


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
    outputStream.Start(mixer);

Console.WriteLine($"Output stream created (dest: {outputMulticast}:5004, {outputConfig.PacketTimeUs/1000}ms/{outputConfig.PacketsPerSecond}pkt/s)\n");

//GET AoIP Input stream (USED AS INPUT IN BASS-RTP)
string inputUrl = $"aes67://{inputMulticast}:5004";
Console.WriteLine($"Creating AES67 input stream... {inputUrl}");
Console.WriteLine("Using direct P/Invoke (bypassing Bass.NET)...");
int toBrowserChan = Aes67Native.BASS_StreamCreateURL_Direct(inputUrl, 0, Aes67Native.BASS_STREAM_DECODE, IntPtr.Zero, IntPtr.Zero);
Console.WriteLine($"BASS_StreamCreateURL (aes67 plugin): {Bass.BASS_ErrorGetCode()}, toBrowserChan={toBrowserChan}");



void DoPeerConnect()
{

    peer = new BassWebRtcPeer(
        signalingUrl: "ws://localhost:8080",
        roomId: "studio-1",
        sourceChannel: toBrowserChan,
        decodeStream: true //,   // <-- This sets DECODE on InputStreamHandle
       // channels: 1,           // Mono
      //  opusBitrate: 64       // 64 kbps (good for voice)
    );

    // Attach event handlers to the NEW peer
    peer.Connected += () =>
    {
        Console.WriteLine("Connected!");
        peer.SetupStreams();
        peer.EnableStats(1000); // Stats every 1 second
        int fromBrowserChan = peer.InputStreamHandle;
        BassMix.BASS_Mixer_StreamAddChannel(mixer, fromBrowserChan, BASSFlag.BASS_STREAM_AUTOFREE);
    };

    
    peer.Disconnected += () =>
    {
        Console.WriteLine("Disconnected - recreating peer...");
        peer.Dispose();

        // Recreate and reconnect after a short delay
        Thread.Sleep(500);
        DoPeerConnect();
    };

    peer.Error += (code, msg) => Console.WriteLine($"Error {code}: {msg}");

    peer.StatsUpdated += stats =>
    {
        Console.WriteLine($"RTT: {stats.RoundTripTime.TotalMilliseconds:F1}ms");
        Console.WriteLine($"Loss: {stats.PacketLossPercent:F2}%");
        Console.WriteLine($"Bitrate: {stats.SendBitrateKbps:F0} kbps");
    };

    // Start the connection
    peer.Connect();
}

DoPeerConnect();



Console.ReadLine();