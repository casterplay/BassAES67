## BASS Structures


## Currest BASS Structure (Simplified)

This will play an AES67 network stream to th edefault soundcard.

```
BASS_Init(-1) //Soundcard using default audio device.
BASS_PluginLoad("aes_67") 

int chan = BASS_StreamCreateURL("aes67://some_address)
bool ret = BASS_ChannelPlay(chan,false)
```


## Real World senario with multiple in and one out, no soundcard.

```
BASS_SetConfig(BASS_CONFIG_BUFFER, 20)
BASS_SetConfig(BASS_CONFIG_UPDATEPERIOD, 0)
BASS_Init(0) //No Soundcard device
BASS_PluginLoad("aes_67") 

//Create a Mixer
int mixer = BASS_Mixer_StreamCreate(48000, 2, BASS_STREAM_DECODE | BASS_MIXER_NONSTOP);

//Create channel "1"
int chan1 = BASS_StreamCreateURL("aes67://239.192.1.10")

//Add the channel to the Mixer
BASS_Mixer_StreamAddChannel(mixer, chan1, BASS_STREAM_AUTOFREE /*| BASS_MIXER_BUFFER*/);

//Create a second channel "2"
int chan2 = BASS_StreamCreateURL("aes67://239.192.1.11")

//Add sewcond  channel to the Mixer
BASS_Mixer_StreamAddChannel(mixer, chan2, BASS_STREAM_AUTOFREE /*| BASS_MIXER_BUFFER*/);


//Add the mixer channel to our (future) AES67 sender
AES67_Sender(mixer, "aes67://239.192.1.100", NORMAL) //Normal = 200 pkt/s RTP

```

## Real World senario with multiple in and multiple out, no soundcard.

```
BASS_SetConfig(BASS_CONFIG_BUFFER, 20)
BASS_SetConfig(BASS_CONFIG_UPDATEPERIOD, 0)
BASS_Init(0) //No Soundcard device
BASS_PluginLoad("aes_67") 

//Create a Mixer
int mixer = BASS_Mixer_StreamCreate(48000, 2, BASS_STREAM_DECODE | BASS_MIXER_NONSTOP);

//Create channel "1"
int chan1 = BASS_StreamCreateURL("aes67://239.192.1.10")

//Add the channel to the Mixer
BASS_Mixer_StreamAddChannel(mixer, chan1, BASS_STREAM_AUTOFREE /*| BASS_MIXER_BUFFER*/);

//Create a second channel "2"
int chan2 = BASS_StreamCreateURL("aes67://239.192.1.11")

//Add sewcond channel to the Mixer
BASS_Mixer_StreamAddChannel(mixer, chan2, BASS_STREAM_AUTOFREE /*| BASS_MIXER_BUFFER*/);

//Play an file
int chan3 = BASS_StreamCreateFile(filename, 0, 0, BASS_FLOAT | BASS_STREAM_DECODE);

//Add sewcond  channel to the Mixer
BASS_Mixer_StreamAddChannel(mixer, chan3, BASS_STREAM_AUTOFREE /*| BASS_MIXER_BUFFER*/);


//Split the mixerstream so we can send muliple outputs
int splitA = BassMix.BASS_Split_StreamCreate(mixer, BASSFlag.BASS_STREAM_DECODE, null);
int splitB = BassMix.BASS_Split_StreamCreate(mixer, BASSFlag.BASS_STREAM_DECODE, null);

//Add the splitA channel to our (future) AES67 sender
AES67_Sender(splitA, "aes67://239.192.1.100", NORMAL) //Normal = 200 pkt/s RTP

//Add stream encoder (send to Icecast server) (simplified)
HENCODE encoder = BASS_Encode_Start(splitB, "lame -r -s 44100 -b 128 -", BASS_ENCODE_NOHEAD, NULL, 0); // setup the encoder
BASS_Encode_CastInit(encoder, "server.com:8000", "password", BASS_ENCODE_TYPE_MP3, "name", "url", "genre", NULL, NULL, 128, BASS_ENCODE_CAST_PUBLIC);

```

## PTP clocking

As for now the PTP mechanism is in the "aes_67" plugin. Should each "aes_67" have it's own PTP mechanism or should that be one "DLL" that all "aes_67" and the future "aes_67_send" uses?


## Audio Output to Soundcard

- Use "BASS_ChannelGetData" to get PCM data from a channel
- Feed that PCM float array into a Rust crate "cpal" which plays to an audio device.
- Syncronised with "bass-ptp" (and BASS_ATTRIB_FREQ)
- https://crates.io/crates/cpal


## YOU must drive the timing, not BASS
- Is a high presissiion timer enough stable or is a PLL neede that is adjusted by PTP?