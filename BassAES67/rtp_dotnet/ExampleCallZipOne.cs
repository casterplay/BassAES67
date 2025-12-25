/*
 * Example: Calling Z/IP ONE from your application
 *
 * This example shows how to use BassRtpInput to connect TO Z/IP ONE.
 * You provide an audio source (e.g., AES67 input, file, mixer) and receive
 * the return audio from Z/IP ONE.
 *
 * Usage scenario:
 * - Your app wants to send audio TO a Z/IP ONE device
 * - You specify the Z/IP ONE IP address and port (9150-9153)
 * - You receive the return audio (what Z/IP ONE sends back)
 *
 * Z/IP ONE reciprocal ports:
 * - 9150: Codec negotiation / lowest bitrate reply
 * - 9151: Lowest bitrate reply
 * - 9152: Same codec reply (most common)
 * - 9153: Highest quality reply
 */

#if false  // Example code - change to #if true to compile

using System.Runtime.InteropServices;
using Un4seen.Bass;
using Un4seen.Bass.AddOn.Mix;

// Configuration
string zipOneIp = "192.168.1.100";   // Z/IP ONE IP address
ushort zipOnePort = 9152;            // Port 9152 = same codec reply
string interfaceIp = "192.168.1.10"; // Your network interface

// Initialize BASS
var audioEngine = new AudioEngine();
audioEngine.InitBass(0);

// Create a mixer as the audio source (you could also use a file, AES67 input, etc.)
int mixer = BassMix.BASS_Mixer_StreamCreate(48000, 2,
    BASSFlag.BASS_STREAM_DECODE | BASSFlag.BASS_SAMPLE_SOFTWARE | BASSFlag.BASS_MIXER_NONSTOP);

// Add your audio sources to the mixer...
// BassMix.BASS_Mixer_StreamAddChannel(mixer, someAudioSource, BASSFlag.BASS_DEFAULT);

// Configure RTP Input (we call Z/IP ONE)
var config = BassRtpInputNative.RtpInputConfigFFI.CreateDefault(zipOneIp, zipOnePort, interfaceIp);
config.SendCodec = BassRtpNative.BASS_RTP_CODEC_G711;  // G.711 is widely compatible
config.ReturnBufferMs = 200;
config.DecodeStream = 1;  // Enable mixer compatibility for return audio

// Create and start the RTP Input stream
using var rtpInput = new BassRtpInput(mixer, config);

if (!rtpInput.Start())
{
    Console.WriteLine($"Failed to start RTP Input: {BassRtpInputNative.BASS_ErrorGetCode()}");
    return;
}

Console.WriteLine($"Connected to Z/IP ONE at {zipOneIp}:{zipOnePort}");
Console.WriteLine("Sending audio and receiving return...");

// Get the return stream (audio coming back from Z/IP ONE)
int returnStream = rtpInput.ReturnStreamHandle;
if (returnStream != 0)
{
    // Option 1: Play directly
    // Bass.BASS_ChannelPlay(returnStream, false);

    // Option 2: Add to a playback mixer (since we used DecodeStream = 1)
    int playbackMixer = BassMix.BASS_Mixer_StreamCreate(48000, 2,
        BASSFlag.BASS_SAMPLE_SOFTWARE | BASSFlag.BASS_MIXER_NONSTOP);
    BassMix.BASS_Mixer_StreamAddChannel(playbackMixer, returnStream, BASSFlag.BASS_DEFAULT);
    Bass.BASS_ChannelPlay(playbackMixer, false);
}

// Monitor statistics
var timer = new System.Timers.Timer(1000);
timer.Elapsed += (s, e) =>
{
    var stats = rtpInput.GetStats();
    double ppm = BassRtpInputNative.GetPpm(stats.CurrentPpmX1000);
    string codec = BassRtpInputNative.GetPayloadTypeName(stats.DetectedReturnPt);

    Console.Write($"\rTX:{stats.TxPackets,6} RX:{stats.RxPackets,6} Buf:{stats.BufferLevel,5} " +
                  $"Drop:{stats.RxDropped} Codec:{codec} PPM:{ppm:+0.0;-0.0}   ");
};
timer.Start();

Console.WriteLine("\nPress Enter to disconnect...");
Console.ReadLine();

rtpInput.Stop();
Console.WriteLine("Disconnected.");

#endif
