To answer your question about GPS-synced GrandMaster PTP and NTP: No, NTP synchronization is NOT required for audio to work correctly. Here's why:
PTP frequency vs absolute time: Your bass_aes67 plugin extracts the frequency (rate) from PTP Sync messages. This works regardless of whether your system clock matches the grandmaster's absolute time. The plugin uses frequency correction (the Freq: -6.62ppm value) to adjust sample rates, not wall-clock alignment.
Delta is about phase, not rate: The increasing Delta (δ 1683.7µs) shows your system clock drifting from the grandmaster's absolute time. But since you're doing rate compensation (adjusting playback/capture speed), not phase alignment (jumping samples to match timestamps), this drift doesn't affect audio quality.
What a GPS grandmaster would improve:
More stable frequency reference (atomic-level precision)
Potentially smaller Delta values if your system clock is also GPS/NTP synced
Better inter-device synchronization if you have multiple AES67 endpoints
When you WOULD need NTP:
If you wanted system logs to have accurate timestamps matching the broadcast network
If you needed to correlate events across multiple machines by wall-clock time
For display purposes (showing "real" time matching broadcast operations)
Bottom line: Your current setup with Freq: -6.62ppm [LOCKED] means the audio rate compensation is working correctly. The Delta drift is cosmetic - it shows your PC's system clock wandering, but the audio samples are being resampled at the correct rate regardless.



Clock Stats Explained
δ (Delta) - Offset from Master
What it is: The time difference between your local clock and the PTP master clock, in microseconds.
Value	Meaning
< ±10µs	Excellent - broadcast quality
< ±100µs	Good - typical for AES67
< ±1000µs (1ms)	Acceptable - may have occasional glitches
> ±1ms	Poor - likely to cause audio artifacts

For end users: "How accurately are we synchronized?" Lower is better. AES67 spec requires < ±1µs for professional use, but ±100µs is fine for most applications.
Delay - Network Path Delay
What it is: The measured round-trip time to the PTP master, divided by 2 (one-way delay estimate).
Value	Meaning
< 100µs	Excellent - direct switch connection
< 500µs	Good - typical LAN
< 2ms	Acceptable - multiple switches
> 5ms	Poor - too many hops or congested network

For end users: "How far away is the master clock?" This reflects network topology. High delay suggests network congestion or too many switch hops.
Freq (Frequency) - Clock Drift Correction
What it is: How much your local crystal oscillator differs from the master, in parts-per-million (ppm).
Value	Meaning
< ±5 ppm	Excellent - high quality oscillator
< ±25 ppm	Good - typical computer hardware
< ±100 ppm	Acceptable - cheap hardware
> ±100 ppm	Poor - hardware issue or unstable

For end users: "How much is my hardware clock drifting?" This is mostly informational - the PTP client compensates for this automatically. Stable values (not jumping around) are more important than the absolute number.
What Good Stats Look Like

Slave to: PTP/0050c2fffe901131:1, δ 15.2µs, Delay: 45.0µs, Freq: +3.21ppm [LOCKED] | STABLE