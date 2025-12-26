//! NDI sender wrapper for video and audio transmission.

use std::sync::Arc;
use grafton_ndi::{NDI, Sender, SenderOptions, PixelFormat};
use thiserror::Error;

use crate::frame::{AudioFrame, VideoFrame, VideoFormat};

/// Errors that can occur during NDI operations
#[derive(Error, Debug)]
pub enum NdiError {
    #[error("Failed to initialize NDI: {0}")]
    InitError(String),

    #[error("Failed to create sender: {0}")]
    SenderError(String),

    #[error("Failed to send frame: {0}")]
    SendError(String),

    #[error("Unsupported video format: {0:?}")]
    UnsupportedFormat(VideoFormat),
}

/// NDI sender for transmitting video and audio.
///
/// This wraps the grafton-ndi Sender with proper lifetime management.
pub struct NdiSender<'a> {
    /// NDI instance (must be kept alive)
    ndi: Arc<NDI>,
    /// The actual sender
    sender: Sender<'a>,
    /// Source name
    name: String,
}

impl<'a> NdiSender<'a> {
    /// Create a new NDI sender with the given source name.
    ///
    /// The source name will be visible in NDI receivers/monitors.
    pub fn new(ndi: &'a Arc<NDI>, name: &str) -> Result<Self, NdiError> {
        // Create sender options
        let options = SenderOptions::builder(name)
            .clock_video(true) // Let NDI handle timing
            .build();

        // Create sender
        let sender = Sender::new(ndi.as_ref(), &options)
            .map_err(|e| NdiError::SenderError(e.to_string()))?;

        Ok(Self {
            ndi: ndi.clone(),
            sender,
            name: name.to_string(),
        })
    }

    /// Get the source name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Send a video frame.
    ///
    /// The frame will be transmitted to all connected NDI receivers.
    pub fn send_video(&self, frame: &VideoFrame) -> Result<(), NdiError> {
        // Convert our format to NDI PixelFormat
        let pixel_format = match frame.format {
            VideoFormat::BGRA => PixelFormat::BGRA,
            VideoFormat::BGRX => PixelFormat::BGRX,
            VideoFormat::RGBA => PixelFormat::RGBA,
            VideoFormat::RGBX => PixelFormat::RGBX,
            VideoFormat::UYVY => PixelFormat::UYVY,
            VideoFormat::NV12 => PixelFormat::NV12,
            VideoFormat::I420 => PixelFormat::I420,
        };

        // Build the NDI video frame with our data
        let mut ndi_frame = grafton_ndi::VideoFrame::builder()
            .resolution(frame.width as i32, frame.height as i32)
            .frame_rate(frame.frame_rate_n as i32, frame.frame_rate_d as i32)
            .pixel_format(pixel_format)
            .build()
            .map_err(|e| NdiError::SendError(e.to_string()))?;

        // Copy our frame data into the NDI frame (data field is public)
        let copy_len = ndi_frame.data.len().min(frame.data.len());
        ndi_frame.data[..copy_len].copy_from_slice(&frame.data[..copy_len]);

        // Send the frame
        self.sender.send_video(&ndi_frame);

        Ok(())
    }

    /// Send an audio frame.
    ///
    /// Audio samples should be interleaved f32 values.
    pub fn send_audio(&self, frame: &AudioFrame) -> Result<(), NdiError> {
        // Build the NDI audio frame with our data included
        let ndi_frame = grafton_ndi::AudioFrame::builder()
            .sample_rate(frame.sample_rate as i32)
            .channels(frame.channels as i32)
            .samples(frame.samples_per_channel as i32)
            .data(frame.data.clone())
            .build()
            .map_err(|e| NdiError::SendError(e.to_string()))?;

        // Send the frame
        self.sender.send_audio(&ndi_frame);

        Ok(())
    }

    /// Check if there are any connected receivers
    pub fn has_connections(&self) -> bool {
        self.sender.connection_count(std::time::Duration::ZERO).unwrap_or(0) > 0
    }

    /// Get the number of connected receivers
    pub fn connection_count(&self) -> u32 {
        self.sender.connection_count(std::time::Duration::ZERO).unwrap_or(0)
    }
}

/// Initialize the NDI library and return an Arc-wrapped instance.
///
/// This should be called once at application startup.
pub fn init_ndi() -> Result<Arc<NDI>, NdiError> {
    let ndi = NDI::new().map_err(|e| NdiError::InitError(e.to_string()))?;
    Ok(Arc::new(ndi))
}
