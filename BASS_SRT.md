### IDEA

"bass_srt" library will contain two parts, an "input module" and an "output" module. 

- The "input module" is a :BASS Plugin" that receives a SRT stream and feed the received PCM audio into a BASS channel. For this you can reference the "bass_aes67" input "/home/kennet/dev/BassAES67/BassAES67/bass-aes67/src/input"

- The "output module" is a application (a lib) that gets a BASS channel, excracts the PCM data and send it voa SRT. For that you can look at the "aes_67" output: "/home/kennet/dev/BassAES67/BassAES67/bass-aes67/src/output"

- Clocking. If needed, "aes_67" already have a clocking module.

Please use the "bass_srt" folder for this project


## SRT reference ###
Git: https://github.com/Haivision/srt
Git Documentation: https://github.com/Haivision/srt/tree/master/docs#documentation-overview

- Initaly we are intresed in the simplest form of "live transmitt" and "live receive"
- Start simple with just sending/receiving PCM audio as L16, 48khz, 2 channels
- Later on we should add OPUS codec for send/receive, then use latest OPUS 1.6 https://opus-codec.org/release/stable/2025/12/15/libopus-1_6.html
- When all works will look into "SRT Connection Bonding: Socket Groups"


