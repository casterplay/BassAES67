The sample rate of a channel.

BASS_ChannelSetAttribute(
    DWORD handle,
    BASS_ATTRIB_FREQ,
    float freq
);
Parameters
handle	The channel handle.
freq	The sample rate... 0 = original rate (when the channel was created).
Remarks
This attribute is applied during playback of the channel and is not present in the sample data returned by BASS_ChannelGetData, so it has no direct effect on decoding channels.
Increasing the sample rate of a stream or MOD music increases its CPU usage, and reduces the length of its playback buffer in terms of time. If you intend to raise the sample rate above the original rate then you may also need to increase the buffer length via the BASS_CONFIG_BUFFER config option to avoid breaks in the sound.

When using BASS_ChannelSlideAttribute to slide this attribute, the BASS_SLIDE_LOG flag can be used to make a slide logarithmic rather than linear.

See also
BASS_ChannelGetAttribute, BASS_ChannelSetAttribute, BASS_ChannelSlideAttribute, BASS_GetInfo



Sets the value of a channel's attribute.

BOOL BASS_ChannelSetAttribute(
    DWORD handle,
    DWORD attrib,
    float value
);
Parameters
handle	The channel handle... a HCHANNEL, HMUSIC, HSTREAM, or HRECORD.
attrib	The attribute to set the value of. One of the following.
BASS_ATTRIB_BUFFER	Playback buffering length.
BASS_ATTRIB_DOWNMIX	Playback downmixing.
BASS_ATTRIB_FREQ	Sample rate.
BASS_ATTRIB_GRANULE	Processing granularity.
BASS_ATTRIB_MUSIC_AMPLIFY	MOD music amplification level.
BASS_ATTRIB_MUSIC_BPM	MOD music BPM.
BASS_ATTRIB_MUSIC_PANSEP	MOD music pan separation level.
BASS_ATTRIB_MUSIC_PSCALER	MOD music position scaler.
BASS_ATTRIB_MUSIC_SPEED	MOD music speed.
BASS_ATTRIB_MUSIC_VOL_CHAN	MOD music channel volume level.
BASS_ATTRIB_MUSIC_VOL_GLOBAL	MOD music global volume level.
BASS_ATTRIB_MUSIC_VOL_INST	MOD music instrument/sample volume level.
BASS_ATTRIB_NET_RESUME	Download buffer level to resume stalled playback.
BASS_ATTRIB_NORAMP	Playback ramping.
BASS_ATTRIB_PAN	Panning/balance position.
BASS_ATTRIB_PUSH_LIMIT	Push stream buffer limit.
BASS_ATTRIB_SRC	Sample rate conversion quality.
BASS_ATTRIB_TAIL	Length extension.
BASS_ATTRIB_VOL	Volume level.
BASS_ATTRIB_VOLDSP	DSP chain volume level.
BASS_ATTRIB_VOLDSP_PRIORITY	DSP chain volume priority.
Other attributes may be supported by add-ons.
value	The new attribute value. See the attribute's documentation for details on the possible values.
Return value
If successful, then TRUE is returned, else FALSE is returned. Use BASS_ErrorGetCode to get the error code.
Error codes
BASS_ERROR_HANDLE	handle is not a valid channel.
BASS_ERROR_ILLTYPE	The channel does not have the requested attribute.
BASS_ERROR_ILLPARAM	value is not valid.
BASS_ERROR_DENIED	The attribute is read-only.
Remarks
When setting an integer attribute, the floating-point value will be truncated (fractional part removed) and capped to integer range, but values exceeding +/-16777216 may be imprecise. BASS_ChannelSetAttributeEx can be used instead to set such large values precisely.
If the attribute is currently sliding from a BASS_ChannelSlideAttribute call then that will be stopped before applying the new value. BASS_SYNC_SLIDE syncs will be triggered by this.

See also
BASS_ChannelFlags, BASS_ChannelGetAttribute, BASS_ChannelSetAttributeEx, BASS_ChannelSet3DAttributes, BASS_ChannelSlideAttribute