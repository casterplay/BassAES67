using System.Net;

/// <summary>
/// Configuration for AES67 output stream - all parameters settable
/// Mirrors the Rust Aes67OutputConfig struct
/// </summary>
public class Aes67OutputConfig
{
    /// <summary>Multicast destination address</summary>
    public IPAddress MulticastAddr { get; set; } = IPAddress.Parse("239.192.76.52");

    /// <summary>UDP port</summary>
    public ushort Port { get; set; } = 5004;

    /// <summary>Network interface to send from (null = default interface)</summary>
    public IPAddress? Interface { get; set; }

    /// <summary>RTP payload type (typically 96 for L24/48000)</summary>
    public byte PayloadType { get; set; } = 96;

    /// <summary>Number of audio channels</summary>
    public ushort Channels { get; set; } = 2;

    /// <summary>Sample rate in Hz</summary>
    public uint SampleRate { get; set; } = 48000;

    /// <summary>Packet time in microseconds (250, 1000, or 5000 for Livewire)</summary>
    public uint PacketTimeUs { get; set; } = 1000;  // 1ms default (AES67 standard)

    /// <summary>Calculated samples per packet based on sample rate and packet time</summary>
    public int SamplesPerPacket => (int)(SampleRate * PacketTimeUs / 1_000_000);

    /// <summary>Total samples per packet including all channels</summary>
    public int TotalSamplesPerPacket => SamplesPerPacket * Channels;

    /// <summary>Calculated payload size in bytes (L24 = 3 bytes per sample)</summary>
    public int PayloadSize => TotalSamplesPerPacket * 3;

    /// <summary>Total RTP packet size (12 byte header + payload)</summary>
    public int PacketSize => 12 + PayloadSize;

    /// <summary>Packets per second</summary>
    public int PacketsPerSecond => (int)(1_000_000 / PacketTimeUs);
}
