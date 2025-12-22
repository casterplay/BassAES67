using System.Runtime.InteropServices;

namespace BlazorServerApp.Services;

/// <summary>
/// P/Invoke declarations for bass_opus_web native library.
/// </summary>
public static class BassOpusWebInterop
{
    private const string DllName = "bass_opus_web";

    /// <summary>
    /// Callback delegate - called from Rust for each Opus frame.
    /// </summary>
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void OpusFrameCallback(
        IntPtr data,
        uint len,
        ulong timestampMs,
        IntPtr user
    );

    /// <summary>
    /// Encoder configuration structure.
    /// </summary>
    [StructLayout(LayoutKind.Sequential)]
    public struct EncoderConfig
    {
        public uint SampleRate;
        public ushort Channels;
        public uint BitrateKbps;
        public byte Reserved;
    }

    /// <summary>
    /// Encoder statistics structure.
    /// </summary>
    [StructLayout(LayoutKind.Sequential)]
    public struct EncoderStats
    {
        public ulong FramesEncoded;
        public ulong SamplesProcessed;
        public ulong Underruns;
        public ulong CallbackErrors;
    }

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    public static extern void BASS_OPUS_WEB_SetCallback(OpusFrameCallback callback, IntPtr user);

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    public static extern void BASS_OPUS_WEB_ClearCallback();

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    public static extern IntPtr BASS_OPUS_WEB_Create(uint bassChannel, ref EncoderConfig config);

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    public static extern int BASS_OPUS_WEB_Start(IntPtr handle);

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    public static extern int BASS_OPUS_WEB_Stop(IntPtr handle);

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    public static extern int BASS_OPUS_WEB_IsRunning(IntPtr handle);

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    public static extern int BASS_OPUS_WEB_GetStats(IntPtr handle, out EncoderStats stats);

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    public static extern int BASS_OPUS_WEB_Free(IntPtr handle);
}
