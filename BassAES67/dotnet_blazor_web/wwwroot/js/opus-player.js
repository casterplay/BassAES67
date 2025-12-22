// Opus WebCodecs Player
// Receives Opus frames from WebSocket, decodes with WebCodecs, plays via Web Audio API

class OpusPlayer {
    constructor() {
        this.audioContext = null;
        this.decoder = null;
        this.webSocket = null;
        this.isPlaying = false;
        this.scheduledTime = 0;
        this.sampleRate = 48000;
        this.channels = 2;
        this.frameCount = 0;
    }

    // Start playback and connect to WebSocket
    async start(wsUrl) {
        try {
            // Create AudioContext (48kHz to match Opus)
            this.audioContext = new AudioContext({ sampleRate: this.sampleRate });

            // Resume AudioContext if suspended (browser autoplay policy)
            if (this.audioContext.state === 'suspended') {
                await this.audioContext.resume();
            }

            // Create WebCodecs AudioDecoder
            this.decoder = new AudioDecoder({
                output: (audioData) => this.handleDecodedAudio(audioData),
                error: (e) => console.error('[OpusPlayer] Decoder error:', e)
            });

            // Configure for Opus 48kHz stereo
            await this.decoder.configure({
                codec: 'opus',
                sampleRate: this.sampleRate,
                numberOfChannels: this.channels
            });

            // Connect to WebSocket
            return new Promise((resolve, reject) => {
                this.webSocket = new WebSocket(wsUrl);
                this.webSocket.binaryType = 'arraybuffer';

                this.webSocket.onopen = () => {
                    console.log('[OpusPlayer] WebSocket connected');
                    this.isPlaying = true;
                    this.scheduledTime = this.audioContext.currentTime + 0.1;
                    this.frameCount = 0;
                    resolve(true);
                };

                this.webSocket.onmessage = (event) => {
                    if (event.data instanceof ArrayBuffer) {
                        this.handleOpusFrame(new Uint8Array(event.data));
                    }
                };

                this.webSocket.onerror = (error) => {
                    console.error('[OpusPlayer] WebSocket error:', error);
                    reject(error);
                };

                this.webSocket.onclose = () => {
                    console.log('[OpusPlayer] WebSocket closed');
                    this.isPlaying = false;
                };
            });
        } catch (error) {
            console.error('[OpusPlayer] Start failed:', error);
            await this.stop();
            return false;
        }
    }

    // Handle incoming Opus frame from WebSocket
    handleOpusFrame(opusData) {
        if (!this.decoder || this.decoder.state !== 'configured') {
            return;
        }

        try {
            // Calculate timestamp from frame count (5ms per frame)
            const timestampUs = this.frameCount * 5000;
            this.frameCount++;

            // Create EncodedAudioChunk from Opus data
            const chunk = new EncodedAudioChunk({
                type: 'key',
                timestamp: timestampUs,
                data: opusData
            });

            this.decoder.decode(chunk);
        } catch (error) {
            console.error('[OpusPlayer] Decode error:', error);
        }
    }

    // Handle decoded audio data from WebCodecs
    handleDecodedAudio(audioData) {
        if (!this.audioContext || !this.isPlaying) {
            audioData.close();
            return;
        }

        try {
            // Create AudioBuffer from decoded data
            const buffer = this.audioContext.createBuffer(
                audioData.numberOfChannels,
                audioData.numberOfFrames,
                audioData.sampleRate
            );

            // Copy decoded samples to buffer (channel by channel)
            for (let ch = 0; ch < audioData.numberOfChannels; ch++) {
                const channelData = new Float32Array(audioData.numberOfFrames);
                audioData.copyTo(channelData, { planeIndex: ch });
                buffer.copyToChannel(channelData, ch);
            }

            // Schedule playback
            const source = this.audioContext.createBufferSource();
            source.buffer = buffer;
            source.connect(this.audioContext.destination);

            // Ensure continuous playback - skip ahead if we've fallen behind
            const now = this.audioContext.currentTime;
            if (this.scheduledTime < now) {
                this.scheduledTime = now + 0.05;
            }

            source.start(this.scheduledTime);
            this.scheduledTime += buffer.duration;

        } catch (error) {
            console.error('[OpusPlayer] Playback error:', error);
        } finally {
            audioData.close();
        }
    }

    // Stop playback and disconnect
    async stop() {
        this.isPlaying = false;

        if (this.webSocket) {
            try {
                this.webSocket.close();
            } catch (e) {
                // Ignore close errors
            }
            this.webSocket = null;
        }

        if (this.decoder) {
            try {
                this.decoder.close();
            } catch (e) {
                // Ignore close errors
            }
            this.decoder = null;
        }

        if (this.audioContext) {
            try {
                await this.audioContext.close();
            } catch (e) {
                // Ignore close errors
            }
            this.audioContext = null;
        }

        console.log('[OpusPlayer] Stopped');
    }

    // Get current playback state
    getState() {
        return {
            isPlaying: this.isPlaying,
            webSocketState: this.webSocket?.readyState ?? -1,
            audioContextState: this.audioContext?.state || 'closed',
            framesReceived: this.frameCount
        };
    }
}

// Export for use in Blazor
window.OpusPlayer = OpusPlayer;

// Helper function for Blazor interop
window.createOpusPlayer = function() {
    return new OpusPlayer();
};
