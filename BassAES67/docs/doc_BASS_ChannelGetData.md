BASS_ChannelGetData
Retrieves the immediate sample data (or an FFT representation of it) of a sample channel, stream, MOD music, or recording channel.

DWORD BASS_ChannelGetData(
    DWORD handle,
    void *buffer,
    DWORD length
);
Parameters
handle	The channel handle... a HCHANNEL, HMUSIC, HSTREAM, or HRECORD.
buffer	Pointer to a buffer to receive the data... can be NULL when handle is a recording channel (HRECORD), to discard the requested amount of data from the recording buffer.
length	Number of bytes wanted (up to 268435455 or 0xFFFFFFF), and/or the following flags.
BASS_DATA_FLOAT	Return floating-point sample data.
BASS_DATA_FFT256	256 sample FFT (returns 128 values).
BASS_DATA_FFT512	512 sample FFT (returns 256 values).
BASS_DATA_FFT1024	1024 sample FFT (returns 512 values).
BASS_DATA_FFT2048	2048 sample FFT (returns 1024 values).
BASS_DATA_FFT4096	4096 sample FFT (returns 2048 values).
BASS_DATA_FFT8192	8192 sample FFT (returns 4096 values).
BASS_DATA_FFT16384	16384 sample FFT (returns 8192 values).
BASS_DATA_FFT32768	32768 sample FFT (returns 16384 values).
BASS_DATA_FFT_COMPLEX	Return the complex FFT result rather than the magnitudes. This increases the amount of data returned (as listed above) fourfold, as it returns real and imaginary parts and the full FFT result (not only the first half). The real and imaginary parts are interleaved in the returned data.
BASS_DATA_FFT_INDIVIDUAL	Perform a separate FFT for each channel, rather than a single combined FFT. The size of the data returned (as listed above) is multiplied by the number of channels.
BASS_DATA_FFT_NOWINDOW	Prevent a Hann window being applied to the sample data when performing an FFT.
BASS_DATA_FFT_NYQUIST	Return an extra value for the Nyquist frequency magnitude. The Nyquist frequency is always included in a complex FFT result.
BASS_DATA_FFT_REMOVEDC	Remove any DC bias from the sample data when performing an FFT.
BASS_DATA_NOREMOVE	Do not remove the data from a recording channel's buffer. This also prevents the channel's DSP/FX being applied to the data, and is automatic if the recording channel is using a RECORDPROC callback function.
BASS_DATA_AVAILABLE	Query the amount of data the channel has buffered for playback, or from recording. This flag cannot be used with decoding channels as they do not have playback buffers. buffer must be NULL when using this flag.
Return value
If an error occurs, -1 is returned, use BASS_ErrorGetCode to get the error code. When requesting FFT data, the number of bytes read from the channel (to perform the FFT) is returned. When requesting sample data, the number of bytes written to buffer will be returned (not necessarily the same as the number of bytes read when using the BASS_DATA_FLOAT flag). When using the BASS_DATA_AVAILABLE flag, the number of bytes in the channel's buffer is returned.
Error codes
BASS_ERROR_HANDLE	handle is not a valid channel.
BASS_ERROR_ENDED	The channel has reached the end.
BASS_ERROR_NOTAVAIL	The BASS_DATA_AVAILABLE flag cannot be used with a decoding channel. It is not possible to get data from final output mix streams (using STREAMPROC_DEVICE).
BASS_ERROR_ILLPARAM	Invalid flags were used, eg. BASS_DATA_NOREMOVE on a non-recording channel.
Remarks
Unless the channel is a decoding channel, this function can only return as much data as has been written to the channel's playback buffer, so it may not always be possible to get the amount of data requested, especially if it is a large amount. If larger amounts are needed, the buffer length (BASS_CONFIG_BUFFER) can be increased. The BASS_DATA_AVAILABLE flag can be used to check how much data a channel's buffer contains at any time, including when stopped or stalled. BASS will retain some extra data beyond the configured buffer length to account for device latency and give the data that is currently being heard, so the amount of available data can actually exceed the configured buffer length.
When requesting data from a decoding channel, data is decoded directly from the channel's source (no playback buffer) and as much data as the channel has available can be decoded at a time.

When retrieving sample data, 8-bit samples are unsigned (0 to 255), 16-bit samples are signed (-32768 to 32767), 32-bit floating-point samples range from -1 to +1 (not clipped, so can actually be outside this range). That is unless the BASS_DATA_FLOAT flag is used, in which case the sample data will be converted to 32-bit floating-point (if it is not already).

Unless complex data is requested via the BASS_DATA_FFT_COMPLEX flag, the magnitudes of the first half of an FFT result are returned. For example, with a 2048 sample FFT (BASS_DATA_FFT2048), there will be 1024 floating-point values returned. Each value, or "bin", ranges from 0 to 1 (can actually go higher if the sample data is floating-point and not clipped). The 1st bin contains the DC component, the 2nd contains the amplitude at 1/2048 of the channel's sample rate, followed by the amplitude at 2/2048, 3/2048, etc. A Hann window is applied to the sample data to reduce leakage, unless the BASS_DATA_FFT_NOWINDOW flag is used. When a window is applied, it causes the DC component to leak into the next bin, but that can be removed (reduced to 0) by using the BASS_DATA_FFT_REMOVEDC flag. Doing so slightly increases the processing required though, so it should only be done when needed, which is when a window is applied and the 2nd bin value is important.

Channels that have 2 or more sample channels (ie. stereo or above) may have FFT performed on each individual channel, using the BASS_DATA_FFT_INDIVIDUAL flag. Without this flag, all of the channels are combined, and a single mono FFT is performed. Performing the extra individual FFTs of course increases the amount of processing required. The return values are interleaved in the same order as the channel's sample data, eg. stereo = left,right,left,etc.

This function is most useful if you wish to visualize (eg. spectrum analyze) the sound.

Example
Perform a 1024 sample FFT on a channel and list the result.
float fft[512]; // fft data buffer
BASS_ChannelGetData(channel, fft, BASS_DATA_FFT1024);
for (int a = 0; a < 512; a++)
    printf("%d: %f\n", a, fft[a]);
Perform a 1024 sample FFT on a channel and list the complex result.

float fft[2048]; // fft data buffer
BASS_ChannelGetData(channel, fft, BASS_DATA_FFT1024 | BASS_DATA_FFT_COMPLEX);
for (int a = 0; a < 1024; a++)
    printf("%d: (%f, %f)\n", a, fft[a * 2], fft[a * 2 + 1]);
See also
BASS_ChannelGetLevel, BASS_ChannelIsActive