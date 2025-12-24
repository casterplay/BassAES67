//! G.722 wideband audio codec encoder and decoder.
//!
//! G.722 is a wideband audio codec operating at 16 kHz sample rate.
//! It uses sub-band ADPCM encoding at 64 kbps (8 bits per sample).
//!
//! Payload type: PT 9
//!
//! Algorithm sourced from ezk-media (public domain / MIT license):
//! https://github.com/kbalt/ezk-media
//!
//! Original implementation by Steve Underwood (SpanDSP) based on
//! CMU's ADPCM implementation.

use super::{AudioDecoder, AudioEncoder, CodecError};

/// G.722 bitrate modes.
#[derive(Clone, Copy)]
pub enum Bitrate {
    /// 64 kbps (8 bits per sample) - standard mode
    Mode1_64000,
    /// 56 kbps (7 bits per sample)
    Mode2_56000,
    /// 48 kbps (6 bits per sample)
    Mode3_48000,
}

impl Bitrate {
    fn bits_per_sample(&self) -> i32 {
        match self {
            Bitrate::Mode1_64000 => 8,
            Bitrate::Mode2_56000 => 7,
            Bitrate::Mode3_48000 => 6,
        }
    }
}

impl Default for Bitrate {
    fn default() -> Self {
        Bitrate::Mode1_64000
    }
}

/// G.722 band state for ADPCM processing.
#[derive(Default)]
struct G722Band {
    s: i32,
    sp: i32,
    sz: i32,
    r: [i32; 3],
    a: [i32; 3],
    ap: [i32; 3],
    p: [i32; 3],
    d: [i32; 7],
    b: [i32; 7],
    bp: [i32; 7],
    sg: [i32; 7],
    nb: i32,
    det: i32,
}

/// Internal decoder state.
struct DecoderState {
    packed: bool,
    eight_k: bool,
    bits_per_sample: i32,
    x: [i32; 24],
    band: [G722Band; 2],
    in_buffer: u32,
    in_bits: i32,
}

impl DecoderState {
    fn new(rate: Bitrate, packed: bool, eight_k: bool) -> Self {
        Self {
            packed,
            eight_k,
            bits_per_sample: rate.bits_per_sample(),
            x: [0; 24],
            band: Default::default(),
            in_buffer: 0,
            in_bits: 0,
        }
    }
}

/// G.722 decoder.
///
/// Decodes G.722 encoded audio to f32 PCM samples at 16 kHz.
/// This is a stateful decoder - the ADPCM algorithm maintains state
/// between decode calls.
pub struct G722Decoder {
    state: DecoderState,
    channels: u8,
}

impl G722Decoder {
    /// Create a new G.722 decoder with default settings for RTP.
    ///
    /// RTP G.722 uses 64 kbps (8 bits per sample). Testing shows packed mode
    /// works correctly for Z/IP ONE.
    pub fn new() -> Self {
        Self {
            // Try packed mode - Z/IP ONE may use this
            state: DecoderState::new(Bitrate::Mode1_64000, true, false),
            channels: 1,
        }
    }

    /// Create a new G.722 decoder with specified channel count.
    pub fn with_channels(channels: u8) -> Self {
        Self {
            state: DecoderState::new(Bitrate::Mode1_64000, true, false),
            channels,
        }
    }

    /// Create a new G.722 decoder with custom settings.
    ///
    /// # Arguments
    /// * `rate` - Bitrate mode (64/56/48 kbps)
    /// * `packed` - Whether input is bit-packed (true) or one code per byte (false)
    /// * `eight_k` - Output at 8 kHz (true) or 16 kHz (false)
    pub fn with_options(rate: Bitrate, packed: bool, eight_k: bool) -> Self {
        Self {
            state: DecoderState::new(rate, packed, eight_k),
            channels: 1,
        }
    }
}

impl Default for G722Decoder {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioDecoder for G722Decoder {
    /// Decode G.722 encoded data to f32 samples.
    ///
    /// G.722 decodes to 16 kHz mono, but we upsample to 48 kHz stereo for compatibility
    /// with the 48 kHz stereo stream. Each input byte produces 2 samples at 16 kHz,
    /// which becomes 12 samples at 48 kHz stereo (3x upsample, 2x for stereo).
    fn decode(&mut self, data: &[u8], output: &mut [f32]) -> Result<usize, CodecError> {
        // Decode to i16 samples at 16 kHz
        let samples_16k = g722_decode(&mut self.state, data);

        // We need 3x upsampling * 2 channels = 6 output samples per 16kHz sample
        let output_samples = samples_16k.len() * 6;

        if output.len() < output_samples {
            return Err(CodecError::BufferTooSmall);
        }

        // Convert i16 to f32, upsample 16kHz -> 48kHz (3x), and duplicate to stereo
        let mut out_idx = 0;
        for &sample in samples_16k.iter() {
            let f32_sample = sample as f32 / 32768.0;
            // Replicate each sample 3 times for 16kHz -> 48kHz, with L+R stereo pairs
            for _ in 0..3 {
                output[out_idx] = f32_sample;     // Left
                output[out_idx + 1] = f32_sample; // Right
                out_idx += 2;
            }
        }

        Ok(output_samples)
    }

    /// Frame size in samples per channel (20ms at 48kHz after upsampling = 960 samples).
    fn frame_size(&self) -> usize {
        960 // 320 samples at 16kHz * 3 = 960 at 48kHz
    }

    /// Total samples per frame including all channels (stereo output).
    fn total_samples_per_frame(&self) -> usize {
        960 * 2 // Always stereo output
    }
}

// ============================================================================
// G.722 Decoder Algorithm (from SpanDSP / CMU)
// ============================================================================

/// Saturate a 32-bit value to 16-bit range.
#[inline]
fn saturate(amp: i32) -> i32 {
    amp.clamp(i16::MIN as i32, i16::MAX as i32)
}

/// Block 4 processing - ADPCM filter update.
fn block4(band: &mut G722Band, d: i32) {
    let mut wd1: i32;
    let mut wd2: i32;
    let mut wd3: i32;
    let mut i: usize;

    // Block 4, RECONS
    band.d[0] = d;
    band.r[0] = saturate(band.s + d);

    // Block 4, PARREC
    band.p[0] = saturate(band.sz + d);

    // Block 4, UPPOL2
    for i in 0..3 {
        band.sg[i] = band.p[i] >> 15;
    }
    wd1 = saturate(band.a[1] << 2);

    wd2 = if band.sg[0] == band.sg[1] { -wd1 } else { wd1 };
    if wd2 > 32767 {
        wd2 = 32767;
    }
    wd3 = (wd2 >> 7) + (if band.sg[0] == band.sg[2] { 128 } else { -128 });
    wd3 += (band.a[2] * 32512) >> 15;
    wd3 = wd3.clamp(-12288, 12288);
    band.ap[2] = wd3;

    // Block 4, UPPOL1
    band.sg[0] = band.p[0] >> 15;
    band.sg[1] = band.p[1] >> 15;
    wd1 = if band.sg[0] == band.sg[1] { 192 } else { -192 };
    wd2 = (band.a[1] * 32640) >> 15;

    band.ap[1] = saturate(wd1 + wd2);
    wd3 = saturate(15360 - band.ap[2]);
    if band.ap[1] > wd3 {
        band.ap[1] = wd3;
    } else if band.ap[1] < -wd3 {
        band.ap[1] = -wd3;
    }

    // Block 4, UPZERO
    wd1 = if d == 0 { 0 } else { 128 };
    band.sg[0] = d >> 15;
    i = 1;
    while i < 7 {
        band.sg[i] = band.d[i] >> 15;
        wd2 = if band.sg[i] == band.sg[0] { wd1 } else { -wd1 };
        wd3 = (band.b[i] * 32640) >> 15;
        band.bp[i] = saturate(wd2 + wd3);
        i += 1;
    }

    // Block 4, DELAYA
    i = 6;
    while i > 0 {
        band.d[i] = band.d[i - 1];
        band.b[i] = band.bp[i];
        i -= 1;
    }
    i = 2;
    while i > 0 {
        band.r[i] = band.r[i - 1];
        band.p[i] = band.p[i - 1];
        band.a[i] = band.ap[i];
        i -= 1;
    }

    // Block 4, FILTEP
    wd1 = saturate(band.r[1] + band.r[1]);
    wd1 = (band.a[1] * wd1) >> 15;
    wd2 = saturate(band.r[2] + band.r[2]);
    wd2 = (band.a[2] * wd2) >> 15;
    band.sp = saturate(wd1 + wd2);

    // Block 4, FILTEZ
    band.sz = 0;
    i = 6;
    while i > 0 {
        wd1 = saturate(band.d[i] + band.d[i]);
        band.sz += (band.b[i] * wd1) >> 15;
        i -= 1;
    }
    band.sz = saturate(band.sz);

    // Block 4, PREDIC
    band.s = saturate(band.sp + band.sz);
}

/// Decode G.722 data to PCM samples.
fn g722_decode(s: &mut DecoderState, g722_data: &[u8]) -> Vec<i16> {
    // Lookup tables
    static WL: [i32; 8] = [-60, -30, 58, 172, 334, 538, 1198, 3042];
    static RL42: [i32; 16] = [0, 7, 6, 5, 4, 3, 2, 1, 7, 6, 5, 4, 3, 2, 1, 0];
    static ILB: [i32; 32] = [
        2048, 2093, 2139, 2186, 2233, 2282, 2332, 2383, 2435, 2489, 2543, 2599, 2656, 2714, 2774,
        2834, 2896, 2960, 3025, 3091, 3158, 3228, 3298, 3371, 3444, 3520, 3597, 3676, 3756, 3838,
        3922, 4008,
    ];
    static WH: [i32; 3] = [0, -214, 798];
    static RH2: [i32; 4] = [2, 1, 2, 1];
    static QM2: [i32; 4] = [-7408, -1616, 7408, 1616];
    static QM4: [i32; 16] = [
        0, -20456, -12896, -8968, -6288, -4240, -2584, -1200, 20456, 12896, 8968, 6288, 4240, 2584,
        1200, 0,
    ];
    static QM5: [i32; 32] = [
        -280, -280, -23352, -17560, -14120, -11664, -9752, -8184, -6864, -5712, -4696, -3784,
        -2960, -2208, -1520, -880, 23352, 17560, 14120, 11664, 9752, 8184, 6864, 5712, 4696, 3784,
        2960, 2208, 1520, 880, 280, -280,
    ];
    static QM6: [i32; 64] = [
        -136, -136, -136, -136, -24808, -21904, -19008, -16704, -14984, -13512, -12280, -11192,
        -10232, -9360, -8576, -7856, -7192, -6576, -6000, -5456, -4944, -4464, -4008, -3576, -3168,
        -2776, -2400, -2032, -1688, -1360, -1040, -728, 24808, 21904, 19008, 16704, 14984, 13512,
        12280, 11192, 10232, 9360, 8576, 7856, 7192, 6576, 6000, 5456, 4944, 4464, 4008, 3576,
        3168, 2776, 2400, 2032, 1688, 1360, 1040, 728, 432, 136, -432, -136,
    ];
    static QMF_COEFFS: [i32; 12] = [3, -11, 12, 32, -210, 951, 3876, -805, 362, -156, 53, -11];

    let mut dlowt: i32;
    let mut rlow: i32;
    let mut ihigh: i32;
    let mut dhigh: i32;
    let mut rhigh: i32;
    let mut xout1: i32;
    let mut xout2: i32;
    let mut wd1: i32;
    let mut wd2: i32;
    let mut wd3: i32;
    let mut code: i32;
    let mut i: usize;
    let mut j: usize;

    let mut out = Vec::with_capacity(g722_data.len() * 2);

    rhigh = 0;
    j = 0;
    while j < g722_data.len() {
        if s.packed {
            // Unpack the code bits
            if s.in_bits < s.bits_per_sample {
                s.in_buffer |= (g722_data[j] as u32) << s.in_bits;
                j += 1;
                s.in_bits += 8;
            }
            code = (s.in_buffer & ((1 << s.bits_per_sample) - 1) as u32) as i32;
            s.in_buffer >>= s.bits_per_sample;
            s.in_bits -= s.bits_per_sample;
        } else {
            code = g722_data[j] as i32;
            j += 1;
        }

        match s.bits_per_sample {
            7 => {
                wd1 = code & 0x1f;
                ihigh = (code >> 5) & 0x3;
                wd2 = QM5[wd1 as usize];
                wd1 >>= 1;
            }
            6 => {
                wd1 = code & 0xf;
                ihigh = (code >> 4) & 0x3;
                wd2 = QM4[wd1 as usize];
            }
            _ => {
                wd1 = code & 0x3f;
                ihigh = (code >> 6) & 0x3;
                wd2 = QM6[wd1 as usize];
                wd1 >>= 2;
            }
        }

        // Block 5L, LOW BAND INVQBL
        wd2 = (s.band[0].det * wd2) >> 15;

        // Block 5L, RECONS
        rlow = s.band[0].s + wd2;

        // Block 6L, LIMIT
        rlow = rlow.clamp(-16384, 16383);

        // Block 2L, INVQAL
        wd2 = QM4[wd1 as usize];
        dlowt = (s.band[0].det * wd2) >> 15;

        // Block 3L, LOGSCL
        wd2 = RL42[wd1 as usize];
        wd1 = (s.band[0].nb * 127) >> 7;
        wd1 += WL[wd2 as usize];
        wd1 = wd1.clamp(0, 18432);
        s.band[0].nb = wd1;

        // Block 3L, SCALEL
        wd1 = (s.band[0].nb >> 6) & 31;
        wd2 = 8 - (s.band[0].nb >> 11);
        wd3 = if wd2 < 0 {
            ILB[wd1 as usize] << -wd2
        } else {
            ILB[wd1 as usize] >> wd2
        };
        s.band[0].det = wd3 << 2;

        block4(&mut s.band[0], dlowt);

        if !s.eight_k {
            // Block 2H, INVQAH
            wd2 = QM2[ihigh as usize];
            dhigh = (s.band[1].det * wd2) >> 15;

            // Block 5H, RECONS
            rhigh = dhigh + s.band[1].s;

            // Block 6H, LIMIT
            rhigh = rhigh.clamp(-16384, 16383);

            // Block 2H, INVQAH
            wd2 = RH2[ihigh as usize];
            wd1 = (s.band[1].nb * 127) >> 7;
            wd1 += WH[wd2 as usize];
            wd1 = wd1.clamp(0, 22528);
            s.band[1].nb = wd1;

            // Block 3H, SCALEH
            wd1 = (s.band[1].nb >> 6) & 31;
            wd2 = 10 - (s.band[1].nb >> 11);
            wd3 = if wd2 < 0 {
                ILB[wd1 as usize] << -wd2
            } else {
                ILB[wd1 as usize] >> wd2
            };
            s.band[1].det = wd3 << 2;

            block4(&mut s.band[1], dhigh);
        }

        if s.eight_k {
            out.push((rlow << 1) as i16);
        } else {
            // Apply the receive QMF
            for i in 0..22 {
                s.x[i] = s.x[i + 2];
            }
            s.x[22] = rlow + rhigh;
            s.x[23] = rlow - rhigh;

            xout1 = 0;
            xout2 = 0;
            i = 0;
            while i < 12 {
                xout2 += s.x[2 * i] * QMF_COEFFS[i];
                xout1 += s.x[2 * i + 1] * QMF_COEFFS[11 - i];
                i += 1;
            }

            out.push(saturate(xout1 >> 11) as i16);
            out.push(saturate(xout2 >> 11) as i16);
        }
    }

    out
}

// ============================================================================
// G.722 Encoder Algorithm
// ============================================================================

/// Internal encoder state.
struct EncoderState {
    packed: bool,
    bits_per_sample: i32,
    x: [i32; 24],
    band: [G722Band; 2],
    out_buffer: u32,
    out_bits: i32,
}

impl EncoderState {
    fn new(rate: Bitrate, packed: bool) -> Self {
        let mut state = Self {
            packed,
            bits_per_sample: rate.bits_per_sample(),
            x: [0; 24],
            band: Default::default(),
            out_buffer: 0,
            out_bits: 0,
        };
        // Initialize detector values
        state.band[0].det = 32;
        state.band[1].det = 8;
        state
    }
}

/// G.722 encoder.
///
/// Encodes PCM audio samples to G.722 format at 16 kHz.
/// Input is 48kHz stereo, output is 16kHz mono G.722.
pub struct G722Encoder {
    state: EncoderState,
    /// Downsampling state (3:1 ratio from 48kHz to 16kHz)
    downsample_buffer: [f32; 3],
    downsample_idx: usize,
}

impl G722Encoder {
    /// Create a new G.722 encoder with default settings.
    pub fn new() -> Self {
        Self {
            state: EncoderState::new(Bitrate::Mode1_64000, true),
            downsample_buffer: [0.0; 3],
            downsample_idx: 0,
        }
    }

    /// Create a new G.722 encoder with specified bitrate.
    pub fn with_bitrate(rate: Bitrate) -> Self {
        Self {
            state: EncoderState::new(rate, true),
            downsample_buffer: [0.0; 3],
            downsample_idx: 0,
        }
    }
}

impl Default for G722Encoder {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioEncoder for G722Encoder {
    /// Encode f32 samples to G.722.
    ///
    /// Input: 48kHz stereo f32 samples (interleaved L,R,L,R,...)
    /// Output: G.722 encoded bytes (64kbps = 8 bits per 16kHz sample)
    ///
    /// Downsamples 3:1 (48kHz -> 16kHz) and mixes stereo to mono.
    fn encode(&mut self, pcm: &[f32], output: &mut [u8]) -> Result<usize, CodecError> {
        // Input: 48kHz stereo = 48 samples/ms * 2 channels = 96 values/ms
        // Intermediate: 16kHz mono = 16 samples/ms
        // Output: G.722 at 64kbps = 8 bytes/ms (8 bits per 16kHz sample)
        // So 20ms frame: 1920 input samples -> 320 samples at 16kHz -> 160 output bytes

        let stereo_pairs = pcm.len() / 2;

        // Collect 16kHz mono samples first
        let mut samples_16k: Vec<i16> = Vec::with_capacity(stereo_pairs / 3 + 1);

        for i in 0..stereo_pairs {
            let left = pcm[i * 2];
            let right = pcm[i * 2 + 1];
            let mono = (left + right) * 0.5;

            self.downsample_buffer[self.downsample_idx] = mono;
            self.downsample_idx += 1;

            if self.downsample_idx >= 3 {
                // Average the 3 samples for 3:1 downsampling
                let sample = (self.downsample_buffer[0]
                    + self.downsample_buffer[1]
                    + self.downsample_buffer[2])
                    / 3.0;
                self.downsample_idx = 0;

                // Convert to i16
                let sample_i16 = (sample * 32767.0).clamp(-32768.0, 32767.0) as i16;
                samples_16k.push(sample_i16);
            }
        }

        // G.722 produces 1 byte per 2 input samples at 16kHz
        let output_bytes = samples_16k.len() / 2;
        if output.len() < output_bytes {
            return Err(CodecError::BufferTooSmall);
        }

        // Encode to G.722
        let encoded = g722_encode(&mut self.state, &samples_16k);

        let len = encoded.len().min(output.len());
        output[..len].copy_from_slice(&encoded[..len]);

        Ok(len)
    }

    /// Frame size in samples per channel.
    /// 20ms at 48kHz = 960 samples per channel.
    fn frame_size(&self) -> usize {
        960
    }

    /// Total samples per frame (stereo input).
    /// 20ms at 48kHz stereo = 1920 samples.
    fn total_samples_per_frame(&self) -> usize {
        1920
    }

    /// RTP payload type for G.722.
    fn payload_type(&self) -> u8 {
        9
    }
}

/// Encode PCM samples to G.722.
fn g722_encode(s: &mut EncoderState, amp: &[i16]) -> Vec<u8> {
    // Lookup tables (same as decoder)
    static Q6: [i32; 32] = [
        0, 35, 72, 110, 150, 190, 233, 276, 323, 370, 422, 473, 530, 587, 650, 714, 786, 858, 940,
        1023, 1121, 1219, 1339, 1458, 1612, 1765, 1980, 2195, 2557, 2919, 0, 0,
    ];
    static ILN: [i32; 32] = [
        0, 63, 62, 31, 30, 29, 28, 27, 26, 25, 24, 23, 22, 21, 20, 19, 18, 17, 16, 15, 14, 13, 12,
        11, 10, 9, 8, 7, 6, 5, 4, 0,
    ];
    static ILP: [i32; 32] = [
        0, 61, 60, 59, 58, 57, 56, 55, 54, 53, 52, 51, 50, 49, 48, 47, 46, 45, 44, 43, 42, 41, 40,
        39, 38, 37, 36, 35, 34, 33, 32, 0,
    ];
    static WL: [i32; 8] = [-60, -30, 58, 172, 334, 538, 1198, 3042];
    static ILB: [i32; 32] = [
        2048, 2093, 2139, 2186, 2233, 2282, 2332, 2383, 2435, 2489, 2543, 2599, 2656, 2714, 2774,
        2834, 2896, 2960, 3025, 3091, 3158, 3228, 3298, 3371, 3444, 3520, 3597, 3676, 3756, 3838,
        3922, 4008,
    ];
    static QM4: [i32; 16] = [
        0, -20456, -12896, -8968, -6288, -4240, -2584, -1200, 20456, 12896, 8968, 6288, 4240, 2584,
        1200, 0,
    ];
    static QM2: [i32; 4] = [-7408, -1616, 7408, 1616];
    static QMF_COEFFS: [i32; 12] = [3, -11, 12, 32, -210, 951, 3876, -805, 362, -156, 53, -11];
    static WH: [i32; 3] = [0, -214, 798];
    static RH2: [i32; 4] = [2, 1, 2, 1];

    let mut out: Vec<u8> = Vec::with_capacity(amp.len() / 2 + 1);

    let mut j = 0;
    while j < amp.len() {
        // Apply the QMF filter
        for i in 0..22 {
            s.x[i] = s.x[i + 2];
        }
        s.x[22] = amp[j] as i32;
        j += 1;
        s.x[23] = if j < amp.len() { amp[j] as i32 } else { 0 };
        j += 1;

        // QMF filter
        let mut sumeven: i32 = 0;
        let mut sumodd: i32 = 0;
        for i in 0..12 {
            sumodd += s.x[2 * i] * QMF_COEFFS[i];
            sumeven += s.x[2 * i + 1] * QMF_COEFFS[11 - i];
        }

        let xlow = saturate(sumeven >> 13);
        let xhigh = saturate(sumodd >> 13);

        // Low band encode
        let el = saturate(xlow - s.band[0].s);
        let mut wd = el.abs();
        let mut i: usize = 1;
        while i < 30 {
            wd = (wd - (Q6[i as usize] * s.band[0].det) >> 12) as i32;
            if wd < 0 {
                break;
            }
            i += 1;
        }
        let ilow = if el < 0 {
            ILN[i as usize]
        } else {
            ILP[i as usize]
        };

        // Low band INVQAL
        let ril = ilow >> 2;
        let wd2 = QM4[ril as usize];
        let dlowt = (s.band[0].det * wd2) >> 15;

        // Low band RECONS
        let rlow = saturate(s.band[0].s + dlowt);

        // Low band LOGSCL
        let wd = (ilow >> 2) & 15;
        let wd1 = (s.band[0].nb * 127) >> 7;
        let wd = wd1 + WL[wd as usize];
        let wd = wd.clamp(0, 18432);
        s.band[0].nb = wd;

        // Low band SCALEL
        let wd1 = (s.band[0].nb >> 6) & 31;
        let wd2 = 8 - (s.band[0].nb >> 11);
        let wd3 = if wd2 < 0 {
            ILB[wd1 as usize] << -wd2
        } else {
            ILB[wd1 as usize] >> wd2
        };
        s.band[0].det = wd3 << 2;

        block4(&mut s.band[0], dlowt);

        // High band encode
        let eh = saturate(xhigh - s.band[1].s);
        let wd = if eh < 0 { -1 } else { 0 };
        let ihigh = if (eh.abs() - 564 * s.band[1].det >> 12) < 0 {
            if wd != 0 { 3 } else { 1 }
        } else if wd != 0 {
            2
        } else {
            0
        };

        // High band INVQAH
        let wd2 = QM2[ihigh as usize];
        let dhigh = (s.band[1].det * wd2) >> 15;

        // High band RECONS
        let _rhigh = saturate(s.band[1].s + dhigh);

        // High band LOGSCH
        let wd = RH2[ihigh as usize];
        let wd1 = (s.band[1].nb * 127) >> 7;
        let wd = wd1 + WH[wd as usize];
        let wd = wd.clamp(0, 22528);
        s.band[1].nb = wd;

        // High band SCALEH
        let wd1 = (s.band[1].nb >> 6) & 31;
        let wd2 = 10 - (s.band[1].nb >> 11);
        let wd3 = if wd2 < 0 {
            ILB[wd1 as usize] << -wd2
        } else {
            ILB[wd1 as usize] >> wd2
        };
        s.band[1].det = wd3 << 2;

        block4(&mut s.band[1], dhigh);

        // Pack the code
        let code = (ihigh << 6) | ilow;

        if s.packed {
            s.out_buffer |= (code as u32) << s.out_bits;
            s.out_bits += s.bits_per_sample;
            while s.out_bits >= 8 {
                out.push((s.out_buffer & 0xFF) as u8);
                s.out_buffer >>= 8;
                s.out_bits -= 8;
            }
        } else {
            out.push(code as u8);
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decoder_creation() {
        let decoder = G722Decoder::new();
        assert_eq!(decoder.frame_size(), 960); // 320 * 3x upsampling
        assert_eq!(decoder.total_samples_per_frame(), 1920); // 960 * 2 stereo channels
    }

    #[test]
    fn test_decoder_basic() {
        let mut decoder = G722Decoder::new();

        // Create some test data (silence-ish pattern)
        // 160 bytes = 320 samples at 16kHz, then 3x upsampled * 2 stereo = 1920 samples
        let input = [0u8; 160];
        let mut output = [0.0f32; 2048]; // Enough for 160 * 2 * 3 * 2 = 1920 samples

        let samples = decoder.decode(&input, &mut output).unwrap();

        // Should produce 2 samples per input byte at 16kHz, then 3x upsampled * 2 stereo
        // 160 bytes * 2 samples/byte * 3x upsample * 2 channels = 1920 samples
        assert_eq!(samples, 1920);

        // Check outputs are in valid range
        for i in 0..samples {
            assert!(
                output[i] >= -1.0 && output[i] <= 1.0,
                "Sample {} out of range: {}",
                i,
                output[i]
            );
        }
    }

    #[test]
    fn test_saturate() {
        assert_eq!(saturate(0), 0);
        assert_eq!(saturate(32767), 32767);
        assert_eq!(saturate(-32768), -32768);
        assert_eq!(saturate(40000), 32767);
        assert_eq!(saturate(-40000), -32768);
    }
}
