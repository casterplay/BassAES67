//! Video and audio frame types for NDI output.

/// Video pixel format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoFormat {
    /// BGRA 8-bit per channel (32 bits per pixel)
    BGRA,
    /// BGRX 8-bit per channel (32 bits per pixel, alpha ignored)
    BGRX,
    /// RGBA 8-bit per channel (32 bits per pixel)
    RGBA,
    /// RGBX 8-bit per channel (32 bits per pixel, alpha ignored)
    RGBX,
    /// UYVY 4:2:2 (16 bits per pixel)
    UYVY,
    /// NV12 (12 bits per pixel, Y plane + interleaved UV)
    NV12,
    /// I420/YUV420P (12 bits per pixel, separate Y, U, V planes)
    I420,
}

impl VideoFormat {
    /// Bytes per pixel for packed formats, or 0 for planar
    pub fn bytes_per_pixel(&self) -> usize {
        match self {
            VideoFormat::BGRA | VideoFormat::BGRX | VideoFormat::RGBA | VideoFormat::RGBX => 4,
            VideoFormat::UYVY => 2,
            VideoFormat::NV12 | VideoFormat::I420 => 0, // Planar formats
        }
    }

    /// Calculate total buffer size needed for this format
    pub fn buffer_size(&self, width: u32, height: u32) -> usize {
        let w = width as usize;
        let h = height as usize;
        match self {
            VideoFormat::BGRA | VideoFormat::BGRX | VideoFormat::RGBA | VideoFormat::RGBX => {
                w * h * 4
            }
            VideoFormat::UYVY => w * h * 2,
            VideoFormat::NV12 => w * h + (w * h / 2), // Y + UV interleaved
            VideoFormat::I420 => w * h + (w * h / 2), // Y + U + V
        }
    }
}

/// A video frame to send via NDI
#[derive(Clone)]
pub struct VideoFrame {
    /// Frame width in pixels
    pub width: u32,
    /// Frame height in pixels
    pub height: u32,
    /// Pixel format
    pub format: VideoFormat,
    /// Frame data (layout depends on format)
    pub data: Vec<u8>,
    /// Stride (bytes per line) - 0 means tightly packed
    pub stride: u32,
    /// Frame timestamp in 100ns units (0 = use NDI timing)
    pub timestamp: i64,
    /// Frame rate numerator (e.g., 30000 for 29.97fps)
    pub frame_rate_n: u32,
    /// Frame rate denominator (e.g., 1001 for 29.97fps)
    pub frame_rate_d: u32,
}

impl VideoFrame {
    /// Create a new video frame with the given dimensions and format
    pub fn new(width: u32, height: u32, format: VideoFormat) -> Self {
        let size = format.buffer_size(width, height);
        Self {
            width,
            height,
            format,
            data: vec![0u8; size],
            stride: 0,
            timestamp: 0,
            frame_rate_n: 30000,
            frame_rate_d: 1001,
        }
    }

    /// Create a BGRA test pattern (color bars)
    pub fn test_pattern_bars(width: u32, height: u32) -> Self {
        let mut frame = Self::new(width, height, VideoFormat::BGRA);

        // SMPTE color bars: White, Yellow, Cyan, Green, Magenta, Red, Blue, Black
        let colors: [(u8, u8, u8); 8] = [
            (255, 255, 255), // White
            (255, 255, 0),   // Yellow
            (0, 255, 255),   // Cyan
            (0, 255, 0),     // Green
            (255, 0, 255),   // Magenta
            (255, 0, 0),     // Red
            (0, 0, 255),     // Blue
            (0, 0, 0),       // Black
        ];

        let bar_width = width / 8;

        for y in 0..height {
            for x in 0..width {
                let bar_index = (x / bar_width).min(7) as usize;
                let (r, g, b) = colors[bar_index];
                let pixel_offset = ((y * width + x) * 4) as usize;

                // BGRA format
                frame.data[pixel_offset] = b;     // Blue
                frame.data[pixel_offset + 1] = g; // Green
                frame.data[pixel_offset + 2] = r; // Red
                frame.data[pixel_offset + 3] = 255; // Alpha
            }
        }

        frame
    }
}

/// An audio frame to send via NDI
#[derive(Clone)]
pub struct AudioFrame {
    /// Sample rate in Hz (e.g., 48000)
    pub sample_rate: u32,
    /// Number of audio channels
    pub channels: u16,
    /// Number of samples per channel
    pub samples_per_channel: u32,
    /// Interleaved f32 audio samples
    pub data: Vec<f32>,
    /// Timestamp in 100ns units (0 = use NDI timing)
    pub timestamp: i64,
}

impl AudioFrame {
    /// Create a new audio frame
    pub fn new(sample_rate: u32, channels: u16, samples_per_channel: u32) -> Self {
        let total_samples = samples_per_channel as usize * channels as usize;
        Self {
            sample_rate,
            channels,
            samples_per_channel,
            data: vec![0.0f32; total_samples],
            timestamp: 0,
        }
    }
}
