using System.Runtime.InteropServices;

namespace BlazorServerApp.Services;

/// <summary>
/// Manages the Opus encoder and broadcasts frames to WebSocket clients.
/// </summary>
public class AudioEncoderService : IDisposable
{
    private readonly ILogger<AudioEncoderService> logger;
    private IntPtr encoderHandle = IntPtr.Zero;
    private BassOpusWebInterop.OpusFrameCallback? callbackDelegate;
    private bool disposed = false;

    public AudioEncoderService(ILogger<AudioEncoderService> logger)
    {
        this.logger = logger;
    }

    /// <summary>
    /// Start encoding from a BASS channel.
    /// </summary>
    /// <param name="bassChannel">BASS channel handle to encode from.</param>
    /// <param name="bitrateKbps">Opus bitrate in kbps (default 128).</param>
    /// <returns>True if started successfully.</returns>
    public bool Start(uint bassChannel, uint bitrateKbps = 128)
    {
        if (encoderHandle != IntPtr.Zero)
        {
            logger.LogWarning("Encoder already running");
            return false;
        }

        var config = new BassOpusWebInterop.EncoderConfig
        {
            SampleRate = 48000,
            Channels = 2,
            BitrateKbps = bitrateKbps,
            Reserved = 0
        };

        // Register callback BEFORE creating encoder
        // Important: Keep reference to prevent GC
        callbackDelegate = OnOpusFrame;
        BassOpusWebInterop.BASS_OPUS_WEB_SetCallback(callbackDelegate, IntPtr.Zero);

        encoderHandle = BassOpusWebInterop.BASS_OPUS_WEB_Create(bassChannel, ref config);
        if (encoderHandle == IntPtr.Zero)
        {
            logger.LogError("Failed to create encoder");
            BassOpusWebInterop.BASS_OPUS_WEB_ClearCallback();
            callbackDelegate = null;
            return false;
        }

        if (BassOpusWebInterop.BASS_OPUS_WEB_Start(encoderHandle) != 1)
        {
            logger.LogError("Failed to start encoder");
            BassOpusWebInterop.BASS_OPUS_WEB_Free(encoderHandle);
            BassOpusWebInterop.BASS_OPUS_WEB_ClearCallback();
            encoderHandle = IntPtr.Zero;
            callbackDelegate = null;
            return false;
        }

        logger.LogInformation("Encoder started: {BitrateKbps} kbps", bitrateKbps);
        return true;
    }

    /// <summary>
    /// Stop encoding.
    /// </summary>
    public void Stop()
    {
        if (encoderHandle != IntPtr.Zero)
        {
            BassOpusWebInterop.BASS_OPUS_WEB_Stop(encoderHandle);
            BassOpusWebInterop.BASS_OPUS_WEB_Free(encoderHandle);
            encoderHandle = IntPtr.Zero;
            logger.LogInformation("Encoder stopped");
        }
        BassOpusWebInterop.BASS_OPUS_WEB_ClearCallback();
        callbackDelegate = null;
    }

    /// <summary>
    /// Callback from Rust - broadcast to all WebSocket clients.
    /// </summary>
    private void OnOpusFrame(IntPtr data, uint len, ulong timestampMs, IntPtr user)
    {
        // Copy data to managed array
        var buffer = new byte[len];
        Marshal.Copy(data, buffer, 0, (int)len);

        // Broadcast to all connected WebSocket clients (fire-and-forget)
        AudioWebSocketMiddleware.BroadcastFrame(buffer);
    }

    /// <summary>
    /// Check if encoder is running.
    /// </summary>
    public bool IsRunning => encoderHandle != IntPtr.Zero &&
                             BassOpusWebInterop.BASS_OPUS_WEB_IsRunning(encoderHandle) == 1;

    /// <summary>
    /// Get encoder statistics.
    /// </summary>
    public BassOpusWebInterop.EncoderStats GetStats()
    {
        if (encoderHandle == IntPtr.Zero)
            return default;

        BassOpusWebInterop.BASS_OPUS_WEB_GetStats(encoderHandle, out var stats);
        return stats;
    }

    /// <summary>
    /// Get the number of connected WebSocket clients.
    /// </summary>
    public int ClientCount => AudioWebSocketMiddleware.ClientCount;

    public void Dispose()
    {
        if (!disposed)
        {
            Stop();
            disposed = true;
        }
    }
}
