using System.Runtime.InteropServices;
using Un4seen.Bass;
using Un4seen.Bass.AddOn.Mix;

/// <summary>
/// Audio engine using BASS library, Using Bass.NET wrapper
/// See: https://www.radio42.com/bass/help/index.php
/// </summary>

public class AudioEngine
{
    /// <summary>
    /// Get platform-appropriate library name
    /// </summary>
    private static string GetLibraryName(string baseName)
    {
        if (RuntimeInformation.IsOSPlatform(OSPlatform.Windows))
            return $"{baseName}.dll";
        else if (RuntimeInformation.IsOSPlatform(OSPlatform.OSX))
            return $"lib{baseName}.dylib";
        else // Linux and other Unix
            return $"lib{baseName}.so";
    }

    public void InitBass(int deviceId, int freq = 48000)
    {
        BassNet.Registration("kennet@kennet.se", "2X20231816202323");

        Bass.BASS_SetConfig(BASSConfig.BASS_CONFIG_BUFFER, 20);
        Bass.BASS_SetConfig(BASSConfig.BASS_CONFIG_UPDATEPERIOD, 0);

        Bass.BASS_SetConfig(BASSConfig.BASS_CONFIG_NET_PREBUF, 0); // so that we can display the buffering%
        Bass.BASS_SetConfig(BASSConfig.BASS_CONFIG_NET_PLAYLIST, 1);

        Bass.BASS_Init(deviceId, freq, BASSInit.BASS_DEVICE_DEFAULT, IntPtr.Zero);
        if (Bass.BASS_ErrorGetCode() != BASSError.BASS_OK)
        {
            Console.WriteLine($"ERROR - InitBass: {Bass.BASS_ErrorGetCode()}");
            return;
        }

        Console.WriteLine($"OK - InitBass: {Bass.BASS_ErrorGetCode()}");

        var aacLib = GetLibraryName("bass_aac");
        var _plAAC = Bass.BASS_PluginLoad(aacLib);
        if (Bass.BASS_ErrorGetCode() != BASSError.BASS_OK)
        {
            Console.WriteLine($"ERROR - BASS_PluginLoad AAC ({aacLib}): {Bass.BASS_ErrorGetCode()}");
        }

        Console.WriteLine($"OK - BASS_PluginLoad AAC: {Bass.BASS_ErrorGetCode()}");

        // Load AES67 plugin
        var aes67Lib = GetLibraryName("bass_aes67");
        int pluginHandle = Bass.BASS_PluginLoad(aes67Lib);
        if (pluginHandle == 0)
        {
            Console.WriteLine($"ERROR - Failed to load {aes67Lib}: {Bass.BASS_ErrorGetCode()}");
            return;
        }
        Console.WriteLine($"OK - BASS_PluginLoad {aes67Lib}: {Bass.BASS_ErrorGetCode()}");


    }
}
