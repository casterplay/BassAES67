using System.Runtime.InteropServices;

namespace BassWebRtc;

/// <summary>
/// WebRTC constants and P/Invoke declarations for bass_webrtc plugin.
/// Matches the FFI definitions in bass-webrtc/src/lib.rs
/// </summary>
public static class BassWebRtcNative
{
    // =========================================================================
    // Signaling Mode Constants (BASS_WEBRTC_SIGNALING_*)
    // =========================================================================

    /// <summary>Callback-based signaling mode</summary>
    public const byte BASS_WEBRTC_SIGNALING_CALLBACK = 0;

    /// <summary>WHIP HTTP signaling mode (server)</summary>
    public const byte BASS_WEBRTC_SIGNALING_WHIP = 1;

    /// <summary>WHEP HTTP signaling mode (server)</summary>
    public const byte BASS_WEBRTC_SIGNALING_WHEP = 2;

    /// <summary>WHIP client mode (push to external server)</summary>
    public const byte BASS_WEBRTC_SIGNALING_WHIP_CLIENT = 3;

    /// <summary>WHEP client mode (pull from external server)</summary>
    public const byte BASS_WEBRTC_SIGNALING_WHEP_CLIENT = 4;

    // =========================================================================
    // Peer State Constants (PEER_STATE_*)
    // =========================================================================

    /// <summary>Peer is newly created</summary>
    public const uint PEER_STATE_NEW = 0;

    /// <summary>Peer is connecting</summary>
    public const uint PEER_STATE_CONNECTING = 1;

    /// <summary>Peer is connected</summary>
    public const uint PEER_STATE_CONNECTED = 2;

    /// <summary>Peer has disconnected</summary>
    public const uint PEER_STATE_DISCONNECTED = 3;

    /// <summary>Peer connection failed</summary>
    public const uint PEER_STATE_FAILED = 4;

    /// <summary>Peer connection is closed</summary>
    public const uint PEER_STATE_CLOSED = 5;

    // =========================================================================
    // WebRTC Server Configuration Structure
    // Must match WebRtcConfigFFI in lib.rs exactly
    // =========================================================================

    /// <summary>
    /// WebRTC server configuration structure.
    /// </summary>
    [StructLayout(LayoutKind.Sequential)]
    public struct WebRtcConfigFFI
    {
        /// <summary>Sample rate (48000 recommended)</summary>
        public uint SampleRate;

        /// <summary>Number of channels (1 or 2)</summary>
        public ushort Channels;

        /// <summary>OPUS bitrate in kbps (default 128)</summary>
        public uint OpusBitrate;

        /// <summary>Incoming audio buffer in milliseconds (default 100)</summary>
        public uint BufferMs;

        /// <summary>Maximum peers (1-5, default 5)</summary>
        public byte MaxPeers;

        /// <summary>Signaling mode (BASS_WEBRTC_SIGNALING_*)</summary>
        public byte SignalingMode;

        /// <summary>WHIP/WHEP HTTP port (if applicable)</summary>
        public ushort HttpPort;

        /// <summary>Create input stream with BASS_STREAM_DECODE flag (for mixer compatibility)</summary>
        public byte DecodeStream;

        /// <summary>
        /// Create a default configuration.
        /// </summary>
        public static WebRtcConfigFFI CreateDefault(
            uint sampleRate = 48000,
            ushort channels = 2,
            uint opusBitrate = 128,
            uint bufferMs = 100)
        {
            return new WebRtcConfigFFI
            {
                SampleRate = sampleRate,
                Channels = channels,
                OpusBitrate = opusBitrate,
                BufferMs = bufferMs,
                MaxPeers = 5,
                SignalingMode = BASS_WEBRTC_SIGNALING_CALLBACK,
                HttpPort = 8080,
                DecodeStream = 0
            };
        }
    }

    // =========================================================================
    // WebRTC Statistics Structure
    // Must match WebRtcStatsFFI in lib.rs exactly
    // =========================================================================

    /// <summary>
    /// WebRTC statistics structure.
    /// </summary>
    [StructLayout(LayoutKind.Sequential)]
    public struct WebRtcStatsFFI
    {
        /// <summary>Number of active peers</summary>
        public uint ActivePeers;

        /// <summary>Total packets sent</summary>
        public ulong TotalPacketsSent;

        /// <summary>Total packets received</summary>
        public ulong TotalPacketsReceived;

        /// <summary>Total bytes sent</summary>
        public ulong TotalBytesSent;

        /// <summary>Total bytes received</summary>
        public ulong TotalBytesReceived;

        /// <summary>Total encode errors</summary>
        public ulong TotalEncodeErrors;

        /// <summary>Total decode errors</summary>
        public ulong TotalDecodeErrors;

        /// <summary>Output buffer underruns</summary>
        public ulong OutputUnderruns;

        /// <summary>Input buffer level (samples)</summary>
        public uint InputBufferLevel;

        /// <summary>Input is currently buffering (1 = yes, 0 = no)</summary>
        public byte InputIsBuffering;
    }

    // =========================================================================
    // Callback Delegates
    // =========================================================================

    /// <summary>
    /// Delegate for SDP offer/answer callbacks.
    /// </summary>
    /// <param name="peerId">Peer identifier (0-4)</param>
    /// <param name="sdpType">SDP type ("offer" or "answer")</param>
    /// <param name="sdp">SDP content (null-terminated)</param>
    /// <param name="user">User data pointer</param>
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void SdpCallback(uint peerId, IntPtr sdpType, IntPtr sdp, IntPtr user);

    /// <summary>
    /// Delegate for ICE candidate callbacks.
    /// </summary>
    /// <param name="peerId">Peer identifier (0-4)</param>
    /// <param name="candidate">ICE candidate string (null-terminated)</param>
    /// <param name="sdpMid">SDP media ID (null-terminated, may be null)</param>
    /// <param name="sdpMlineIndex">SDP media line index</param>
    /// <param name="user">User data pointer</param>
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void IceCandidateCallback(uint peerId, IntPtr candidate, IntPtr sdpMid, uint sdpMlineIndex, IntPtr user);

    /// <summary>
    /// Delegate for peer state change callbacks.
    /// </summary>
    /// <param name="peerId">Peer identifier (0-4)</param>
    /// <param name="state">New state (PEER_STATE_* constants)</param>
    /// <param name="user">User data pointer</param>
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void PeerStateCallback(uint peerId, uint state, IntPtr user);

    /// <summary>
    /// Signaling callbacks structure.
    /// Must match SignalingCallbacks in callback.rs exactly.
    /// </summary>
    [StructLayout(LayoutKind.Sequential)]
    public struct SignalingCallbacksFFI
    {
        /// <summary>Called when we have an SDP offer/answer to send</summary>
        public IntPtr OnSdp;

        /// <summary>Called when we have an ICE candidate to send</summary>
        public IntPtr OnIceCandidate;

        /// <summary>Called when peer connection state changes</summary>
        public IntPtr OnPeerState;

        /// <summary>User data pointer passed to all callbacks</summary>
        public IntPtr UserData;
    }

    // =========================================================================
    // WebRTC Peer Event Callbacks (for BassWebRtcPeer)
    // =========================================================================

    /// <summary>
    /// Delegate called when WebRTC peer connection is established.
    /// </summary>
    /// <param name="user">User data pointer</param>
    [UnmanagedFunctionPointer(CallingConvention.StdCall)]
    public delegate void OnConnectedCallback(IntPtr user);

    /// <summary>
    /// Delegate called when WebRTC peer connection is closed/disconnected.
    /// </summary>
    /// <param name="user">User data pointer</param>
    [UnmanagedFunctionPointer(CallingConvention.StdCall)]
    public delegate void OnDisconnectedCallback(IntPtr user);

    /// <summary>
    /// Delegate called when a WebRTC peer error occurs.
    /// </summary>
    /// <param name="errorCode">Error code (non-zero)</param>
    /// <param name="errorMsg">Error message (null-terminated string)</param>
    /// <param name="user">User data pointer</param>
    [UnmanagedFunctionPointer(CallingConvention.StdCall)]
    public delegate void OnErrorCallback(uint errorCode, IntPtr errorMsg, IntPtr user);

    /// <summary>
    /// WebRTC peer statistics snapshot (FFI struct).
    /// Matches the Rust WebRtcPeerStatsFFI struct layout.
    /// </summary>
    [StructLayout(LayoutKind.Sequential)]
    public struct WebRtcPeerStatsFFI
    {
        /// <summary>Total packets sent</summary>
        public ulong PacketsSent;
        /// <summary>Total packets received</summary>
        public ulong PacketsReceived;
        /// <summary>Total bytes sent</summary>
        public ulong BytesSent;
        /// <summary>Total bytes received</summary>
        public ulong BytesReceived;
        /// <summary>Round-trip time in milliseconds</summary>
        public uint RoundTripTimeMs;
        /// <summary>Total packets lost (can be negative for duplicates)</summary>
        public long PacketsLost;
        /// <summary>Fraction of packets lost as percentage (0.0 - 100.0)</summary>
        public float FractionLostPercent;
        /// <summary>Jitter in milliseconds</summary>
        public uint JitterMs;
        /// <summary>NACK count (retransmission requests)</summary>
        public ulong NackCount;
    }

    /// <summary>
    /// Delegate called when WebRTC peer statistics are updated.
    /// </summary>
    /// <param name="stats">Pointer to statistics struct</param>
    /// <param name="user">User data pointer</param>
    [UnmanagedFunctionPointer(CallingConvention.StdCall)]
    public delegate void OnStatsCallback(ref WebRtcPeerStatsFFI stats, IntPtr user);

    // =========================================================================
    // Core Server API P/Invoke
    // =========================================================================

    /// <summary>
    /// Create a WebRTC server.
    /// </summary>
    /// <param name="sourceChannel">BASS channel to read audio from (for output to browsers)</param>
    /// <param name="config">Server configuration</param>
    /// <returns>Opaque handle or IntPtr.Zero on failure</returns>
    [DllImport("bass_webrtc", CallingConvention = CallingConvention.StdCall)]
    public static extern IntPtr BASS_WEBRTC_Create(int sourceChannel, ref WebRtcConfigFFI config);

    /// <summary>
    /// Start the WebRTC server.
    /// </summary>
    /// <param name="handle">Handle from BASS_WEBRTC_Create</param>
    /// <returns>1 on success, 0 on failure</returns>
    [DllImport("bass_webrtc", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_WEBRTC_Start(IntPtr handle);

    /// <summary>
    /// Stop the WebRTC server.
    /// </summary>
    /// <param name="handle">Handle from BASS_WEBRTC_Create</param>
    /// <returns>1 on success, 0 on failure</returns>
    [DllImport("bass_webrtc", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_WEBRTC_Stop(IntPtr handle);

    /// <summary>
    /// Free WebRTC server resources.
    /// </summary>
    /// <param name="handle">Handle from BASS_WEBRTC_Create</param>
    /// <returns>1 on success, 0 on failure</returns>
    [DllImport("bass_webrtc", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_WEBRTC_Free(IntPtr handle);

    /// <summary>
    /// Add an ICE server (STUN or TURN).
    /// </summary>
    /// <param name="handle">Handle from BASS_WEBRTC_Create</param>
    /// <param name="url">Server URL (e.g., "stun:stun.l.google.com:19302")</param>
    /// <param name="username">Username for TURN (null for STUN)</param>
    /// <param name="credential">Credential for TURN (null for STUN)</param>
    /// <returns>1 on success, 0 on failure</returns>
    [DllImport("bass_webrtc", CallingConvention = CallingConvention.StdCall, CharSet = CharSet.Ansi)]
    public static extern int BASS_WEBRTC_AddIceServer(IntPtr handle, string url, string? username, string? credential);

    /// <summary>
    /// Set signaling callbacks (for callback mode).
    /// </summary>
    /// <param name="handle">Handle from BASS_WEBRTC_Create</param>
    /// <param name="callbacks">Callback structure</param>
    /// <returns>1 on success, 0 on failure</returns>
    [DllImport("bass_webrtc", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_WEBRTC_SetCallbacks(IntPtr handle, ref SignalingCallbacksFFI callbacks);

    /// <summary>
    /// Add a peer with SDP offer (for callback signaling mode).
    /// </summary>
    /// <param name="handle">Handle from BASS_WEBRTC_Create</param>
    /// <param name="offerSdp">SDP offer from remote peer</param>
    /// <param name="answerSdp">Buffer to receive SDP answer (at least 4096 bytes)</param>
    /// <param name="answerLen">Pointer to receive answer length</param>
    /// <returns>Peer ID (0-4) on success, -1 on failure</returns>
    [DllImport("bass_webrtc", CallingConvention = CallingConvention.StdCall, CharSet = CharSet.Ansi)]
    public static extern int BASS_WEBRTC_AddPeer(IntPtr handle, string offerSdp, IntPtr answerSdp, out uint answerLen);

    /// <summary>
    /// Add an ICE candidate to a peer.
    /// </summary>
    /// <param name="handle">Handle from BASS_WEBRTC_Create</param>
    /// <param name="peerId">Peer ID from BASS_WEBRTC_AddPeer</param>
    /// <param name="candidate">ICE candidate string</param>
    /// <returns>1 on success, 0 on failure</returns>
    [DllImport("bass_webrtc", CallingConvention = CallingConvention.StdCall, CharSet = CharSet.Ansi)]
    public static extern int BASS_WEBRTC_AddIceCandidate(IntPtr handle, uint peerId, string candidate);

    /// <summary>
    /// Remove a peer.
    /// </summary>
    /// <param name="handle">Handle from BASS_WEBRTC_Create</param>
    /// <param name="peerId">Peer ID to remove</param>
    /// <returns>1 on success, 0 on failure</returns>
    [DllImport("bass_webrtc", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_WEBRTC_RemovePeer(IntPtr handle, uint peerId);

    /// <summary>
    /// Get the input stream handle (audio received from browsers).
    /// </summary>
    /// <param name="handle">Handle from BASS_WEBRTC_Create</param>
    /// <returns>BASS stream handle, or 0 if not available</returns>
    [DllImport("bass_webrtc", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_WEBRTC_GetInputStream(IntPtr handle);

    /// <summary>
    /// Get statistics.
    /// </summary>
    /// <param name="handle">Handle from BASS_WEBRTC_Create</param>
    /// <param name="stats">Structure to fill with statistics</param>
    /// <returns>1 on success, 0 on failure</returns>
    [DllImport("bass_webrtc", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_WEBRTC_GetStats(IntPtr handle, out WebRtcStatsFFI stats);

    /// <summary>
    /// Get the number of active peers.
    /// </summary>
    /// <param name="handle">Handle from BASS_WEBRTC_Create</param>
    /// <returns>Number of active peers (0-5)</returns>
    [DllImport("bass_webrtc", CallingConvention = CallingConvention.StdCall)]
    public static extern uint BASS_WEBRTC_GetPeerCount(IntPtr handle);

    /// <summary>
    /// Check if the server is running.
    /// </summary>
    /// <param name="handle">Handle from BASS_WEBRTC_Create</param>
    /// <returns>1 if running, 0 if not</returns>
    [DllImport("bass_webrtc", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_WEBRTC_IsRunning(IntPtr handle);

    // =========================================================================
    // WHIP Client API P/Invoke (Push to external server like MediaMTX)
    // =========================================================================

    /// <summary>
    /// Connect to a WHIP server and push audio.
    /// </summary>
    /// <param name="sourceChannel">BASS channel to read audio from</param>
    /// <param name="whipUrl">WHIP endpoint URL (e.g., "http://localhost:8889/mystream/whip")</param>
    /// <param name="sampleRate">Sample rate (48000 recommended)</param>
    /// <param name="channels">Number of channels (1 or 2)</param>
    /// <param name="opusBitrate">OPUS bitrate in kbps</param>
    /// <returns>Handle on success, IntPtr.Zero on failure</returns>
    [DllImport("bass_webrtc", CallingConvention = CallingConvention.StdCall, CharSet = CharSet.Ansi)]
    public static extern IntPtr BASS_WEBRTC_ConnectWhip(int sourceChannel, string whipUrl, uint sampleRate, ushort channels, uint opusBitrate);

    /// <summary>
    /// Start streaming audio to the connected WHIP server.
    /// </summary>
    /// <param name="handle">Handle from BASS_WEBRTC_ConnectWhip</param>
    /// <returns>1 on success, 0 on failure</returns>
    [DllImport("bass_webrtc", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_WEBRTC_WhipStart(IntPtr handle);

    /// <summary>
    /// Stop streaming and disconnect from the WHIP server.
    /// </summary>
    /// <param name="handle">Handle from BASS_WEBRTC_ConnectWhip</param>
    /// <returns>1 on success, 0 on failure</returns>
    [DllImport("bass_webrtc", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_WEBRTC_WhipStop(IntPtr handle);

    /// <summary>
    /// Free WHIP client resources.
    /// </summary>
    /// <param name="handle">Handle from BASS_WEBRTC_ConnectWhip</param>
    /// <returns>1 on success, 0 on failure</returns>
    [DllImport("bass_webrtc", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_WEBRTC_WhipFree(IntPtr handle);

    /// <summary>
    /// Check if WHIP client is connected.
    /// </summary>
    /// <param name="handle">Handle from BASS_WEBRTC_ConnectWhip</param>
    /// <returns>1 if connected, 0 if not</returns>
    [DllImport("bass_webrtc", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_WEBRTC_WhipIsConnected(IntPtr handle);

    // =========================================================================
    // WHEP Client API P/Invoke (Pull from external server like MediaMTX)
    // =========================================================================

    /// <summary>
    /// Connect to a WHEP server and receive audio.
    /// </summary>
    /// <param name="whepUrl">WHEP endpoint URL (e.g., "http://localhost:8889/mystream/whep")</param>
    /// <param name="sampleRate">Sample rate (48000 recommended)</param>
    /// <param name="channels">Number of channels (1 or 2)</param>
    /// <param name="bufferMs">Buffer size in milliseconds</param>
    /// <param name="decodeStream">Set to 1 for BASS_STREAM_DECODE flag (mixer compatibility)</param>
    /// <returns>Handle on success, IntPtr.Zero on failure</returns>
    [DllImport("bass_webrtc", CallingConvention = CallingConvention.StdCall, CharSet = CharSet.Ansi)]
    public static extern IntPtr BASS_WEBRTC_ConnectWhep(string whepUrl, uint sampleRate, ushort channels, uint bufferMs, byte decodeStream);

    /// <summary>
    /// Get the BASS input stream from a WHEP connection.
    /// </summary>
    /// <param name="handle">Handle from BASS_WEBRTC_ConnectWhep</param>
    /// <returns>BASS stream handle, or 0 if not available</returns>
    [DllImport("bass_webrtc", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_WEBRTC_WhepGetStream(IntPtr handle);

    /// <summary>
    /// Check if WHEP client is connected.
    /// </summary>
    /// <param name="handle">Handle from BASS_WEBRTC_ConnectWhep</param>
    /// <returns>1 if connected, 0 if not</returns>
    [DllImport("bass_webrtc", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_WEBRTC_WhepIsConnected(IntPtr handle);

    /// <summary>
    /// Disconnect from the WHEP server and free resources.
    /// </summary>
    /// <param name="handle">Handle from BASS_WEBRTC_ConnectWhep</param>
    /// <returns>1 on success, 0 on failure</returns>
    [DllImport("bass_webrtc", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_WEBRTC_WhepFree(IntPtr handle);

    // =========================================================================
    // WebSocket Signaling Server API P/Invoke
    // =========================================================================

    /// <summary>
    /// Create a WebSocket signaling server.
    /// The signaling server is a pure WebSocket relay for SDP/ICE exchange.
    /// </summary>
    /// <param name="port">Port to listen on (e.g., 8080)</param>
    /// <returns>Handle on success, IntPtr.Zero on failure</returns>
    [DllImport("bass_webrtc", CallingConvention = CallingConvention.StdCall)]
    public static extern IntPtr BASS_WEBRTC_CreateSignalingServer(ushort port);

    /// <summary>
    /// Start the signaling server.
    /// </summary>
    /// <param name="handle">Handle from BASS_WEBRTC_CreateSignalingServer</param>
    /// <returns>1 on success, 0 on failure</returns>
    [DllImport("bass_webrtc", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_WEBRTC_SignalingServerStart(IntPtr handle);

    /// <summary>
    /// Stop the signaling server.
    /// </summary>
    /// <param name="handle">Handle from BASS_WEBRTC_CreateSignalingServer</param>
    /// <returns>1 on success, 0 on failure</returns>
    [DllImport("bass_webrtc", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_WEBRTC_SignalingServerStop(IntPtr handle);

    /// <summary>
    /// Get the number of connected WebSocket clients.
    /// </summary>
    /// <param name="handle">Handle from BASS_WEBRTC_CreateSignalingServer</param>
    /// <returns>Number of connected clients</returns>
    [DllImport("bass_webrtc", CallingConvention = CallingConvention.StdCall)]
    public static extern uint BASS_WEBRTC_SignalingServerClientCount(IntPtr handle);

    /// <summary>
    /// Free signaling server resources.
    /// </summary>
    /// <param name="handle">Handle from BASS_WEBRTC_CreateSignalingServer</param>
    /// <returns>1 on success, 0 on failure</returns>
    [DllImport("bass_webrtc", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_WEBRTC_SignalingServerFree(IntPtr handle);

    // =========================================================================
    // WebSocket Peer API P/Invoke (Bidirectional WebRTC via Signaling)
    // =========================================================================

    /// <summary>
    /// Create a WebRTC peer that connects via WebSocket signaling with room support.
    /// </summary>
    /// <param name="signalingUrl">WebSocket signaling server base URL (e.g., "ws://localhost:8080")</param>
    /// <param name="roomId">Room identifier for signaling isolation (e.g., "studio-1")</param>
    /// <param name="sourceChannel">BASS channel to send audio from (0 if receive-only)</param>
    /// <param name="sampleRate">Sample rate (48000 recommended)</param>
    /// <param name="channels">Number of channels (1 or 2)</param>
    /// <param name="opusBitrate">OPUS bitrate in kbps (for sending)</param>
    /// <param name="bufferMs">Buffer size in ms for received audio</param>
    /// <param name="decodeStream">Set to 1 for BASS_STREAM_DECODE flag</param>
    /// <returns>Handle on success, IntPtr.Zero on failure</returns>
    [DllImport("bass_webrtc", CallingConvention = CallingConvention.StdCall, CharSet = CharSet.Ansi)]
    public static extern IntPtr BASS_WEBRTC_CreatePeer(
        string signalingUrl,
        string roomId,
        int sourceChannel,
        uint sampleRate,
        ushort channels,
        uint opusBitrate,
        uint bufferMs,
        byte decodeStream);

    /// <summary>
    /// Start connection to the signaling server (non-blocking).
    /// This starts the connection process in the background and returns immediately.
    /// Use BASS_WEBRTC_PeerIsConnected to poll for connection status.
    /// Once connected, call BASS_WEBRTC_PeerSetupStreams to setup audio streams.
    /// </summary>
    /// <param name="handle">Handle from BASS_WEBRTC_CreatePeer</param>
    /// <returns>1 on success (connection started), 0 on failure</returns>
    [DllImport("bass_webrtc", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_WEBRTC_PeerConnect(IntPtr handle);

    /// <summary>
    /// Set callbacks for peer events (connected, disconnected, error).
    /// These callbacks fire when the peer connection state changes.
    /// Call this BEFORE calling BASS_WEBRTC_PeerConnect.
    /// </summary>
    /// <param name="handle">Handle from BASS_WEBRTC_CreatePeer</param>
    /// <param name="onConnected">Callback for connection established (may be null)</param>
    /// <param name="onDisconnected">Callback for connection closed (may be null)</param>
    /// <param name="onError">Callback for errors (may be null)</param>
    /// <param name="user">User data pointer passed to callbacks</param>
    /// <returns>1 on success, 0 on failure</returns>
    [DllImport("bass_webrtc", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_WEBRTC_PeerSetCallbacks(
        IntPtr handle,
        OnConnectedCallback? onConnected,
        OnDisconnectedCallback? onDisconnected,
        OnErrorCallback? onError,
        IntPtr user);

    /// <summary>
    /// Set callback for statistics updates.
    /// When enabled, the callback fires periodically with current statistics.
    /// Call this after creating the peer but before or after connecting.
    /// </summary>
    /// <param name="handle">Handle from BASS_WEBRTC_CreatePeer</param>
    /// <param name="callback">Callback for stats updates (null to disable)</param>
    /// <param name="intervalMs">Interval between updates in milliseconds (e.g., 1000 for 1 second)</param>
    /// <param name="user">User data pointer passed to callback</param>
    /// <returns>1 on success, 0 on failure</returns>
    [DllImport("bass_webrtc", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_WEBRTC_PeerSetStatsCallback(
        IntPtr handle,
        OnStatsCallback? callback,
        uint intervalMs,
        IntPtr user);

    /// <summary>
    /// Check if WebRTC peer is connected.
    /// </summary>
    /// <param name="handle">Handle from BASS_WEBRTC_CreatePeer</param>
    /// <returns>1 if connected, 0 if not</returns>
    [DllImport("bass_webrtc", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_WEBRTC_PeerIsConnected(IntPtr handle);

    /// <summary>
    /// Setup audio streams after connection is established.
    /// Call this after BASS_WEBRTC_PeerIsConnected returns 1.
    /// This sets up the output stream (BASS -> WebRTC) and input stream (WebRTC -> BASS).
    /// </summary>
    /// <param name="handle">Handle from BASS_WEBRTC_CreatePeer</param>
    /// <returns>1 on success, 0 on failure</returns>
    [DllImport("bass_webrtc", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_WEBRTC_PeerSetupStreams(IntPtr handle);

    /// <summary>
    /// Get the BASS input stream from a WebRTC peer (for received audio).
    /// </summary>
    /// <param name="handle">Handle from BASS_WEBRTC_CreatePeer</param>
    /// <returns>BASS stream handle, or 0 if not available</returns>
    [DllImport("bass_webrtc", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_WEBRTC_PeerGetInputStream(IntPtr handle);

    /// <summary>
    /// Disconnect the WebRTC peer.
    /// </summary>
    /// <param name="handle">Handle from BASS_WEBRTC_CreatePeer</param>
    /// <returns>1 on success, 0 on failure</returns>
    [DllImport("bass_webrtc", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_WEBRTC_PeerDisconnect(IntPtr handle);

    /// <summary>
    /// Free WebRTC peer resources.
    /// </summary>
    /// <param name="handle">Handle from BASS_WEBRTC_CreatePeer</param>
    /// <returns>1 on success, 0 on failure</returns>
    [DllImport("bass_webrtc", CallingConvention = CallingConvention.StdCall)]
    public static extern int BASS_WEBRTC_PeerFree(IntPtr handle);

    // =========================================================================
    // BASS Core P/Invoke (convenience, also available via Bass.NET)
    // =========================================================================

    [DllImport("bass")]
    public static extern int BASS_ErrorGetCode();

    // =========================================================================
    // Helper Methods
    // =========================================================================

    /// <summary>
    /// Get peer state name from state constant.
    /// </summary>
    public static string GetPeerStateName(uint state) => state switch
    {
        PEER_STATE_NEW => "New",
        PEER_STATE_CONNECTING => "Connecting",
        PEER_STATE_CONNECTED => "Connected",
        PEER_STATE_DISCONNECTED => "Disconnected",
        PEER_STATE_FAILED => "Failed",
        PEER_STATE_CLOSED => "Closed",
        _ => $"Unknown({state})"
    };

    /// <summary>
    /// Get signaling mode name from mode constant.
    /// </summary>
    public static string GetSignalingModeName(byte mode) => mode switch
    {
        BASS_WEBRTC_SIGNALING_CALLBACK => "Callback",
        BASS_WEBRTC_SIGNALING_WHIP => "WHIP Server",
        BASS_WEBRTC_SIGNALING_WHEP => "WHEP Server",
        BASS_WEBRTC_SIGNALING_WHIP_CLIENT => "WHIP Client",
        BASS_WEBRTC_SIGNALING_WHEP_CLIENT => "WHEP Client",
        _ => $"Unknown({mode})"
    };
}
