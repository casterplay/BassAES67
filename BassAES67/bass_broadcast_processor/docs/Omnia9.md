### Multiband AGC

♦ The Maximum Gain control works in conjunction with the Ratio control to determine how much gain is available below
threshold. If the Input AGC Ratio is set at Infinity:1 and the Input AGC Maximum Gain is set to 36dB, the Input AGC has
36dB of range below threshold. At a ratio of 2.0:1 and the same Maximum gain setting, the range is reduced by half to
18dB. The scale to the left of the Input AGC meter automatically adjusts as needed when changes are made to the Input
AGC Maximum Gain or the Input AGC Ratio to accurately reflect how much range is available below threshold.

♦ The Ratio control determines how much the output audio will be increased or decreased in relationship to the input
audio of the Input AGC section. For example, a ratio of 3:1 means for every 3dB of change in the level of the input audio,
the output will be changed by 1dB. Lower (looser) settings provide less control of the dynamics in this section but
provide a more open sound, while higher (tighter) settings provide more control at the expense of openness. The range
of this control is 1.0:1 to Infinity:1.

♦ The Progressive Release control determines the degree to which the Multiband compressor releases its gain at a faster
rate when the audio is driven further toward or into gain reduction. At a setting of 0, the control has no effect and
the Release speed control fully determines the rate of release both below and above threshold. Increasing the setting
progressively makes the release speed of the audio faster as gain decreases.


### MB AGC Levels

♦ The AGC Target controls set the target gain reduction / output level of the MB AGC bands individually or overall with the
“coupled” control. A lower setting results in more gain reduction thus a corresponding lower output level while higher
settings provide less gain reduction and a higher output level. This is similar to a traditional “threshold” control when the
levels are below the target.

♦ The Band Mix column within the Multiband AGC/MB AGC Levels allows you to adjust the final output of each band
after all Wideband and Multiband processing has been applied. This is the same Band Mix that is a separate tab under the
Processing path. It is provided here as a convenience. It can be used very effectively to tailor the overall spectral balance
of your sound, but keep in mind that this is the final point of adjustment before the audio reaches the Final Clipper (FM
core) or Final Limiter (HD and Streaming Cores). In other words, levels increased in the Band Mix section can only be
controlled by final clipping or final limiting, which may result in unexpected or unwanted density on some material, so
care is required when making adjustments here.

♦ The Drive controls adjust the amount of drive (input level) to the AGC bands individually or overall with the “coupled”
control. Drive differs from Target in that higher settings of Drive actually raise the input level to the AGC band or section,
increasing gain reduction but not changing the output level. Depending on other AGC parameters, the output may
change due to other factors, but the output level set point does not change.


### MB AGC Speed

The Multiband Attack/Release section lets you control the Attack Speed and Release Speed of each band in the multiband AGC
section.

♦ The Attack speed and Release speed controls work in the same manner as their counterparts in the other sections of the
Omnia.9. However, the behavior of the multiband AGC compressors is also program-dependent.

♦ The Attack (coupled) control allows you to adjust the attack speed of all of the bands simultaneously by an equal amount.

♦ The Release (coupled) control allows you to adjust the release speed of all the bands simultaneously by and equal amount.

♦ The Speed (coupled) control allows you to adjust both the Attack speed and Release speed of all the bands simultaneously
by an equal amount.

♦ The Peak Sense (Coupled) control simultaneously adjusts the attack and release rates but in opposition to one another.
Sliding the control to the right increases the attack rate and slows the release rate, making it more peak sensitive. Sliding
the control to the left decreases the attack rate and speeds up the release rate, making it less peak sensitive.
The attack and release speeds of the multiband limiters are program-dependent and not adjustable.

### Ratio Override

♦ The Gain Reduction Ratio Override and Gain Reduction Ratio controls work together to let you set a different ratio for
each band when audio in that band crosses above threshold – that is, when it is driven into gain reduction. The ratio for
audio below threshold is always determined by the Ratio control for all bands. Specifically, the Gain Reduction Ratio
Override control enables or disables the Gain Reduction Ratio controls, which are sliders that let you set the ratio of
audio above threshold from 1:1 to Inf:1. 


### Multiband Compression

# MBC Main

♦ The Maximum Gain control works in conjunction with the Ratio control to determine how much gain is available below
threshold. If the Input AGC Ratio is set at Infinity:1 and the Input AGC Maximum Gain is set to 36dB, the Input AGC has
36dB of range below threshold. At a ratio of 2.0:1 and the same Maximum gain setting, the range is reduced by half to
18dB. The scale to the left of the Input AGC meter automatically adjusts as needed when changes are made to the Input
AGC Maximum Gain or the Input AGC Ratio to accurately reflect how much range is available below threshold.

♦ The Ratio control determines how much the output audio will be increased or decreased in relationship to the input
audio of the Input AGC section. For example, a ratio of 3:1 means for every 3dB of change in the level of the input audio,
the output will be changed by 1dB. Lower (looser) settings provide less control of the dynamics in this section but
provide a more open sound, while higher (tighter) settings provide more control at the expense of openness. The range
of this control is 1.0:1 to Infinity:1.


# MBC Speed

♦ The Attack speed and Release speed controls work in the same manner as their counterparts in the other sections of the
Omnia.9. However, the behavior of the multiband AGC compressors is also program-dependent.

♦ The Attack (coupled) control allows you to adjust the attack speed of all of the bands simultaneously by an equal amount.

♦ The Release (coupled) control allows you to adjust the release speed of all the bands simultaneously by and equal amount.

♦ The Speed (coupled) control allows you to adjust both the Attack speed and Release speed of all the bands simultaneously
by an equal amount.

♦ The Peak Sense (Coupled) control simultaneously adjusts the attack and release rates but in opposition to one another.
Sliding the control to the right increases the attack rate and slows the release rate, making it more peak sensitive. Sliding
the control to the left decreases the attack rate and speeds up the release rate, making it less peak sensitive.
The attack and release speeds of the multiband limiters are program-dependent and not adjustable.

# MBC Delay

The Sidechain Delay feature is useful for both adding punch and managing the amount of low frequency power (while
increasing bass punch). This is an especially useful “trick” for maintaining apparent loudness when operating under MPX
Power regulations. Delay value can be coupled, and set from 0 to 40ms.


# MB Thresholds

The Multiband Thresholds menu allows you to set the target for each of the Multiband AGC bands, target for the compression
for each of the Multiband AGC bands, as well as the threshold for each band of the Multiband Limiters. The total number of
bands available in the Multiband Thresholds section is determined by the number of bands of processing used in the Current
Preset.

♦ The AGC Target controls set the target output level of each band of the Multiband AGC. A lower setting provides a lower
output level, while a higher setting provides a higher output level. These controls have a range between +12 and -12dB in
one-tenth dB increments.

♦ The Comp Target controls set the target compression level for each band of the Multiband AGC. A lower setting provides
less compression, while a higher setting provides additional compression. These controls have a range between +18 and
-3dB in one-quarter dB increments.

♦ The Limiter Threshold controls determine at which point the Multiband Limiter acts upon the incoming audio for its
particular band relative to its corresponding AGC Target. For example, a setting of +6dB means that any peaks of less
6dB above the AGC Target level will not be processed by the limiter. These controls have a range between +18 and 0dB in
one-tenth dB increments.



### Dry Voice Detection (OPTIONAL in out app)

The Dry Voice Detection Menu contains the controls to enable the dry voice detector circuit and adjust the dynamics section of
Omnia.9 when this feature is engaged.
Dry voice is one of the most difficult waveforms to process cleanly, as the human voice is complex in nature and is typically
asymmetrical in form. Stations that choose to process aggressively in an effort to maximize loudness may find that bare vocals
come through with unacceptably high levels of audible distortion.
Omnia.9 overcomes this situation by automatically detecting (in the Auto Detect mode) when the input audio is dry voice
and using a separate set of multiband targets, attack rates, and release rates. This allows the dynamics section to do more of the
“heavy lifting” and reduces the amount of clipping necessary to maintain the same level of loudness.
The Dry Voice dropdown menu can be set to “Off”, “Auto Detect”, or “Force Off” completely turns off this feature. “Auto”
allows the processor to automatically detect the presence of dry voice. “Force” overrides the main multiband settings and
uses the Dry Voice Detection settings at all times. It is useful to force detection of voice while testing with music, in order to
facilitate adjusting the separate dry voice multiband controls so that even when the inevitable false voice detection occurs, the
frequency balance will not unacceptably. It could also be useful to control this parameter by GPI or HTTP, tied to the DJ mic
fader. The default setting depends upon preset chosen; most presets have this featured set to “Off” by default, while the default
setting for more aggressive, loudness-driven presets is “Auto Detect.” 

# Dry Voice Speed

♦ The Attack, Release, Target, Limiter Threshold, Attack (Coupled), Release (Coupled), Speed (Coupled) and Target
(Coupled) controls provide relative adjustments referenced to their counterparts in the Multiband section.

♦ The Peak Sense (Coupled) control simultaneously adjusts the attack and release rates but in opposition to one another.
Sliding the control to the right increases the attack rate and slows the release rate, making it more peak sensitive. Sliding
the control to the left decreases the attack rate and speeds up the release rate, making it less peak sensitive.

♦ The AGC Target for dry voice detection controls works the same way that it does for the MB AGC section, except that
it functions only when dry voice detection is activated, and the values are relative to the corresponding multi-band
target slider. It sets the target gain reduction / output level of the dry voice AGC bands individually or overall with the
“coupled” control. A lower setting results in more gain reduction thus a corresponding lower output level while higher
settings provide less gain reduction and a higher output level. This is similar to a traditional “threshold” control when the
levels are below the target.


### Input AGC Menu

The Input AGC Menu is used to set the ratio, maximum gain, attack rate, release rate, target, gate threshold, freeze threshold,
and sidechain equalizer controls.
The Input AGC is the first gain control stage in Omnia.9 following Undo, and is designed to be used as a slower-acting leveler
ahead of the Wideband AGC1 and multiband compressor sections that follow it.
It is worth noting that traditional processors only act upon audio above a particular threshold. They are driven into various
amounts of gain reduction, but once the audio falls below the threshold, they “run out of room” or “top out,” and are incapable
of increasing the audio any further. They require some sort of make-up gain control later in the audio chain. The compressors
in Omnia.9 operate above AND below threshold, controlling the dynamics over a much wider range and do not require
makeup gain. 

♦ The Ratio control determines how much the output audio will be increased or decreased in relationship to the input
audio of the Input AGC section. For example, a ratio of 3:1 means for every 3dB of change in the level of the input audio,
the output will be changed by 1dB. Lower (looser) settings provide less control of the dynamics in this section but
provide a more open sound, while higher (tighter) settings provide more control at the expense of openness. The range
of this control is 1.0:1 to Infinity:1.
♦ The Maximum Gain control works in conjunction with the Ratio control to determine how much gain is available below
threshold. If the Input AGC Ratio is set at Infinity:1 and the Input AGC Maximum Gain is set to 36dB, the Input AGC has
36dB of range below threshold. At a ratio of 2.0:1 and the same Maximum gain setting, the range is reduced by half to
18dB. The scale to the left of the Input AGC meter automatically adjusts as needed when changes are made to the Input
AGC Maximum Gain or the Input AGC Ratio to accurately reflect how much range is available below threshold.
♦ The Attack control determines the speed with which the Input AGC acts to reduce audio above threshold. Lower settings
represent slower attack speeds and allow more audio to pass unprocessed by the Input AGC into subsequent processing
stages. Higher settings result in faster attack speeds and allow less unprocessed audio to enter subsequent sections.
Because all of Omnia.9’s processing stages are to some extent program-dependent, putting actual measures of time on
these controls would be pointless, and so the numbers on the various Attack and Release controls throughout are simply
relative numbers.
♦ The Release control determines the speed with which the Input AGC increases audio below threshold. Lower settings
provide slower release speeds, while higher settings result in faster release speeds.
♦ The Target control sets the target output level of the Input AGC. A lower setting results in a lower output level, while
higher settings provide a higher output level. This is similar to a traditional “threshold” control when the levels are below
the target.
♦ The Gate Threshold and Freeze Threshold controls work together to determine the points at which the release rate of the
Input AGC slows by a factor of 3 (gate threshold) or freezes altogether (freeze threshold). The range of these controls is
-90dB to 0dB. Lower settings means the audio must drop to a lower level before the release speed slows or freezes. Higher
settings means the audio doesn’t have to drop as much in level before the input AGC gain slows down or stops. Using
higher settings when employing faster Input AGC release speeds can keep the audio from being increased too quickly
or too much during quieter passages or pauses. If the display is sized and configured in such a way that the Input AGC
meter is shown vertically, a Gate condition will be indicated by a dim, dark red bar at the bottom of the meter. A Freeze
condition will be indicated by a brighter dark red bar.

# Sidechain PEQ

♦ The Input AGC features a fully-adjustable, 3-band Sidechain Parametric Equalizer, which allows you to make it more
or less sensitive to particular frequencies. When the controls are not set to cut or boost any frequency, the Input AGC
reacts to the full audio spectrum. When set to cut or boost a particular range of frequencies, the Input AGC becomes less
sensitive (cut) or more sensitive (boost).
♦ The Copy control places the settings into a “clipboard” so that they can be shared by using the Paste control.
♦ The Type drop down menu determines what type of EQ or filter is employed.
♦ The Frequency slider is used to set the center frequency for each band. The range of this control is 20 to 22,050Hz.
♦ The Width slider determines how much audio above and below the center frequency will also be affected by any boosts
or cuts in gain. The range of this control is 0.0 to 10.0 octaves in one-tenth octave increments. Lower values provide a
narrower (sharper) boost or cut, while higher values provide a wider (gentler) boost or cut.
♦ The Gain slider determines how much the audio selected with a combination of the Frequency and Width sliders is
boosted or cut. Each band can be boosted or cut by 12dB in one-quarter dB increments for a total range of 24db per band

### Wideband AGC 1 Menu

The Wideband AGC1 menu provides access to the sidechain delay, maximum gain, maximum gain reduction, ratio, attack,
release, progressive release, target, gate threshold, freeze threshold, and three-band sidechain parametric equalizer controls.
♦ The Wideband AGC1 Enable button enables this section, which follows the Input AGC section and provides additional
wideband compression as determined by its various controls. Disabling the Wideband AGC1 also makes this patch point
unavailable in an oscilloscope or RTA display window.
♦ The Bypass button removes the Wideband AGC1 compressor from the audio path, but its patch point remains an
available option for viewing on the oscilloscope or RTA via the Display Settings menu.
♦ The Sidechain Delay feature is useful for both adding punch and managing the amount of low frequency power (while
increasing bass punch). This is an especially useful “trick” for maintaining apparent loudness when operating under
MPX Power regulations.
♦ The Maximum Gain, Ratio, Attack rate Release rate, Target, Gate Threshold, and Freeze Threshold controls work in the
same manner as their counterparts in the other sections of the Omnia.9. However, the Maximum Gain control in the
Wideband AGC1 section has a range of 24dB.
♦ The Maximum Gain Reduction control sets the maximum amount of gain reduction possible in the Wideband AGC1
compressor, and is adjustable from 0 to 24dB in one-quarter dB increments.
♦ The Progressive Release control determines the degree to which the Wideband AGC1 compressor releases its gain at a
faster rate as the audio is driven further toward or into gain reduction. At a setting of 0, the control has no effect and the
Release speed control fully determines the rate of release. Increasing the setting progressively makes the release speed of
the audio faster as gain decreases.

# Sidechain PEQ

♦ The 3-band Sidechain Equalizer can be used to make the Wideband AGC1 more or less sensitive to the frequencies
determined by the Frequency, Width, and Gain controls, which function exactly like their counterparts in the Input AGC
section above. A PEQ preview patch point similar to the one described in the Input AGC section is also available here.
♦ The Copy control places the settings into a “clipboard” so that they can be shared by using the Paste control. 

### Parametric Equalizer

The Parametric Equalizer menu allows you to set up the 6-band phase-linear parametric equalizer, which is located just ahead
of the multiband section of the processing core. In addition, an assortment of pre-configured filters is available, including a
Low Pass Filter, a High Pass Filter, a Band Pass Filter, a Notch Filter, a Low Shelf EQ, and a High Shelf EQ.

♦ The Bypass button removes the equalizer from the audio path.
♦ The Type drop down menu determines what type of EQ or filter is employed.
♦ The Frequency slider is used to set the center frequency for each band. The range of this control is 20 to 22,050Hz.
♦ The Width slider determines how much audio above and below the center frequency will also be affected by any boosts
or cuts in gain. The range of this control is 0.0 to 10.0 octaves in one-tenth octave increments. Lower values provide a
narrower (sharper) boost or cut, while higher values provide a wider (gentler) boost or cut.
♦ The Gain slider determines how much the audio selected with a combination of the Frequency and Width sliders is
boosted or cut. Each band can be boosted or cut by 12dB in one-quarter dB increments for a total range of 24db per band.
Although changes made in the parametric equalizer section are offset somewhat by the action of the multiband compressors
that follow, this does not occur to the degree you might expect based upon your experience with other processors. The
parametric equalizer in Omnia.9 is a very versatile and powerful tool for creating your on-air sound. A visual representation of
the effects of the PEQ using the built-in real time analyzer can be seen in the RTA portion of the Display Settings section of this
manual.


### Stereo Enhancer

Omnia.9 offers a unique multi-band Stereo Enhancer, whose total number of bands is determined by the number of bands of
processing used in the Current Preset. Regardless, it never works on bass, which is why Band 1 is never represented and Band
2 may be grayed out. This approach significantly reduces the chance that quieter, hard-panned stereo sounds in a recording
with a strong centered mono sound will be shifted out of phase and offers much greater control over the stereo enhancement
available in various portions of the spectrum. A second page allows you to toggle between Main and Spd (speed) controls.
Speed settings adjust attack and release times for each band. 

♦ The Stereo Enhancer menu gives you access to the Target, Maximum Gain, Maximum Gain Reduction, Attack speed, and
Release speed of each of its bands.
♦ The Enable control turns the Stereo Enhancer on or off.
♦ The Target Width control determines the ratio of L+R to L-R. Higher settings provide more stereo enhancement. Adjust
this control carefully to avoid turning the stereo image “inside out” by allowing L-R to overpower L+R which ruins mono
compatibility and increases multipath distortion.
♦ The Maximum Gain control determines how much the Stereo Enhancer can increase L/R separation in an effort to
achieve the Target Width in program material that has a narrow stereo image. The range is between 0 and 18dB.
♦ The Maximum Attenuation control determines how much the Stereo Enhancer can reduce L/R separation in an effort to
achieve the Target Width in program material that already has a wide stereo image. The range is between 0 and 18dB.
♦ The Attack control determines the speed at which the stereo image is narrowed. The Release control determines the
speed at which the stereo image is widened.
♦ The Target Width (coupled), Maximum Gain (coupled), Maximum Attenuation (coupled), Attack (coupled) and
Release (coupled) controls allow you to adjust all the bands simultaneously by an equal amount in their corresponding
sections.



### Multiband Setup 

The Multiband Setup menu provides control over the number of processing bands employed as well as, gate threshold, freeze
threshold, and gate delay controls. Also found here are the enable buttons for gain reduction override and controls for the gain
reduction ratio.

♦ The Band slider determines the number of bands in the multiband processing section and ranges from 2 to 7.
♦ The Gate Threshold, and Freeze Threshold controls work in the same manner as their counterparts in other sections of
the Omnia.9.
♦ The Gate Delay control determines how long the Gate Threshold and Freeze Threshold controls wait before they begin
working. The range of this control is between 0 and 255ms. Setting the control to “0” means that as soon as audio
falls below the threshold as determined by the settings of the Gate and Freeze controls, it immediately slows or stops,
respectively. Higher settings mean it will take longer for the release of the audio to slow or stop. A Gate condition will be
indicated by a dim, dark red bar at the bottom of the multiband meters. A Freeze condition will be indicated by a brighter
dark red bar.
Note:
Setting the Gate Delay much lower than the default setting of 79ms will cause the gate to take effect during
the brief pauses in dry speech, resulting in a much lower volume level from an announcer mic, for instance,
as compared to music. Used creatively, this is actually a very useful tool for controlling announcer/music
level balance.
It is worth mentioning again here the importance of a concept unique to Omnia.9. Most (if not all) other processors “top out
at 0” – that is, they constantly operate in a state of gain reduction, and once the audio falls below threshold, they can no longer
increase it any further. To make up for the fact that they are capable only of reducing gain, they rely upon a “makeup gain”
control somewhere downstream in the audio chain to get the levels back up. Omnia.9 is not only capable of gain reduction –
that is, driving audio levels above threshold as other processors do – but is also capable of increasing gain below threshold.


### Wideband AGC 2

# Main

The Omnia.9’s Wideband AGC2 control allows you to insert one additional AGC processing stage into the chain as outlined in
detail below.
The Wideband AGC2 Main menu provides access to the Sidechain Delay, Maximum Gain, Maximum Gain Reduction, Ratio,
Attack, Release, Progressive Release, Target, Gate Threshold and Freeze Threshold.

♦ The Bypass button removes the Wideband AGC2 compressor from the audio path, but its patch point remains an
available option for viewing on the oscilloscope or RTA via the Display Settings menu.

♦ The Sidechain Delay, Maximum Gain, Maximum Gain Reduction, Ratio, Attack speed, Release speed, Progressive
Release, Target, Gate Threshold, and Freeze Threshold controls work in the same manner as their counterparts in the
Wideband AGC1 section.

♦ The Wideband AGC2 dropdown control enables or disables the Wideband AGC2 section and allows you to choose
whether it is situated before or after the Multiband AGC section or used as a dedicated Bass Compressor.

♦ If you use the AGC2 as a Bass Compressor, it will be situated after the Multiband section but will affect only the lower
bands, and allow you to push the bass a bit harder without over-driving the final clipper or using excessively fast attack
and release speeds in the lower bands of the Multiband AGC.

♦ Bass Only (“BO”) employs a sidechain filter that allows only the audio from the lower bands to affect gain, so only the
lower frequencies are compressed above threshold.

♦ Bass Wideband (“BW”) also employs a sidechain filter, but one that contains the entire audio spectrum, so the bass
becomes more compressed when the entire mix is above threshold. This mode is most useful when loudness is your
primary processing goal, as it could allow full-scale bass audio in circumstances when there is no mid-range or treble
audio present. However, there will be less bass present in situations when there are other frequencies present.

# Sidechain PEQ

A fully-adjustable, 3-band Sidechain Parametric Equalizer is available, which allows you to make it more or less sensitive
to particular frequencies. When the controls are not set to cut or boost any frequency, the Input AGC reacts to the full audio
spectrum. When set to cut or boost a particular range of frequencies, the Input AGC becomes less sensitive (cut) or more
sensitive (boost).


### Wideband AGC 3

# Main

The Wideband AGC3 menu operates in the same manner as Wideband AGC2, with all of the same controls, but with one
difference: It cannot be used as a wideband compressor before the multiband section, only after. It can, however, be used in the
Bass Only or Bass Wideband mode just like Wideband AGC2.

# Sidechain PEQ

Similar to the Wideband AGC 1 and 2, another fully-adjustable, 3-band Sidechain Parametric Equalizer is available here,
which allows you to make it more or less sensitive to particular frequencies. When the controls are not set to cut or boost any
frequency, the Input AGC reacts to the full audio spectrum. When set to cut or boost a particular range of frequencies, the Input
AGC becomes less sensitive (cut) or more sensitive (boost).


### Band Mix

The Band Mix Menu allows you to adjust the final output of each band after all Wideband and Multiband processing has been
applied. It can be used very effectively to tailor the overall spectral balance of your sound, but keep in mind that this is the
final point of adjustment before the audio reaches the Final Clipper (FM and AM cores) or Final Limiter (HD and Streaming
Cores). In other words, levels increased in the Band Mix section can only be controlled by final clipping or final limiting, which
may result in unexpected or unwanted density on some material, so care is required when making adjustments here.

♦ Each Band Level control has a range of -12 to +12dB in one-quarter dB increments.

♦ The Band Mix (coupled) control allows you to adjust the output of all bands in the Band Mix section simultaneously and
by an equal amount.


### Power Limiter

The Power Limiter menu allows you to adjust settings for BS-412 operation. This screen will only appear if you have selected to
run BS-412 power limiting (an ITU standard, mandated by some countries). To enable BS-412, go to Home/System/System
Configuration/Processing Paths and enable MPX Power Control


### Clipper

The Clipper Menu contains controls that allow the sound and texture of the clipper to be fine tuned. Please remember that the
overall clipper drive is determined by the “Input Gain” control in the “Input Adjust” menu. There are 3 screens in the Expert
mode. Use the buttons on the left to navigate between Main, Highs and Lows.

♦ The Final Clip Drive control sets the drive of the final clipper. Decreasing the drive (moving the slider to the left)
reduces the amount of clipping. Conversely, increasing the drive (moving the slider to the right) will result in more
clipping. Less clipping will result in a more open, less-processed, cleaner sound, but at the expense of overall loudness.
More clipping can result in a louder on-air sound – but only up to a point. Even if total dial domination and loudness
are your processing goals, there comes a point when the final wave-form is completely full, and increasing the amount
of clipping will no longer yield additional loudness – only more distortion. We strongly recommend using Omnia.9’s
oscilloscope as well as your ears to monitor the MPX Output signal while adjusting the final clip drive. This control
ranges from -6.0 to +6.0 in one-quarter dB increments, which should give you the (correct) impression that small
changes make a big difference in the sound. Keep in mind that a setting of +0.00 means the control is at the middle of its
range, but does not necessarily mean that no clipping is taking place.

♦ The Bass Clipper Slope control determines the slope characteristics of the bass clipper. 

♦ Slope 1 is filtered at a very low frequency so that the low bass stays “clean” even when clipped hard. However, some of the
mid-bass will pass through the clipper, which may result in more of the final waveform being taken up by the bass. For
lighter processing settings, Slope 1 offers the cleanest and punchiest bass sound.

♦ The Bass Clipper Threshold control sets the threshold of the Bass Clipper. Raising the threshold (moving the slider to
the left) reduces the amount of clipping performed by the Bass Clipper, but place a greater burden on the Final Clipper.
Conversely, lowering the threshold (moving the slider to the right) will yield more bass clipping, which takes some of
the load off the Final Clipper, but may result in low frequency distortion if set too low.

# Clipper Highs Menu

These controls affect the sound and texture of middle and higher frequencies.

♦ Enabling the Sparkling Highs control preserves the openness and enhances the texture of very high frequencies,
especially when the incoming program material has a high degree of high frequency content.

♦ Phat Mids warms up and enhances mid-range frequencies, particularly male and lower female voices. Please note that
“Sparkling Highs” must be enabled for “Phat Mids” to be selectable; if “Phat Mids” is enabled and “Sparkling Highs” is
turned off, the “Phat Mids” indicator will remain yellow and switch back on when “Sparkling Highs” is re-enabled, but
will be grayed out and removed from the audio path until such time.

♦ The Prioritize Mids control emphasizes Mids over Highs when energy levels between the two are equivalent.

♦ The High Frequency Edge control sets tradeoff between highs that are always clean and free from distortion and a
consistent high frequency output at the expense of distortion. A setting of “0” will yield the cleanest highs but with less
overall brightness. A setting of “100” will result in a uniformly bright sound but at the risk of more audible distortion, and
sounds more like the original Omnia.9 clipper when pushed for loudness.

# Clipper Lows Menu

These controls affect the texture and feel of the bass clipper and are sub-divided into two groups:
Bass Intermodulation Protection and Bass Shape.

♦ The controls in the Bass Intermodulation Protection section are primarily responsible for distortion management,
particularly intermod distortion, and for the behavior of the bass clipper. Unless you specifically want the presence of
intermodulation distortion (where strong low frequency material can attenuate or audibly distort higher frequencies)
we strongly recommend the “Intermod Distortion Protection” control be left enabled.

♦ The Clip Threshold control is the point at which bass clipping will occur under normal circumstances and is analogous
to a traditional “bass clipper threshold” control. When bass at the level set by the “Clip Threshold” control would cause
audible IM distortion, it will instead be clipped at the point determined by the “Protection Threshold” control. Note that
if both the “Clip Threshold” and “Protection Threshold” controls are set to the same value, the bass clipper will operate in
the traditional manner with a fixed threshold in all cases.

♦ The Punch Threshold control allows the initial edge of the bass waveform to be clipped at a different threshold for extra
bass punch; the amount of time this threshold is active is determined by the “Punch Duration” slider.

♦ The controls in the Bass Shape menu help set the “texture” of the bass.

♦ Unlike most bass clippers, the one in Omnia.9 can (but does not have to) create a square wave when processing strong
low frequency content. Setting the “Bass Shape Strength” control at 0 will always prevent square waving the bass
resulting in a very smooth low end texture, but one that does not offer the “slam” of a traditional Omnia clipper even
on the initial peak of a bass transient waveform. Advancing the control toward 100 will produce more square waves,
increasing the bass “punch” and overall loudness, but at the possible expense of more distortion in the low end.

♦ The Bass Shape Frequency control sets the frequency of the highest harmonic produced by the bass clipper. Higher
settings will produce more and higher harmonics and create a more complex and frequency-rich bass sound. 