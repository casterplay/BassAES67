/*
    BASS_AES67 2.4 C/C++ header file
    AES67 Network Audio Plugin for BASS
*/

#ifndef BASS_AES67_H
#define BASS_AES67_H

#include "bass.h"

#ifdef __cplusplus
extern "C" {
#endif

// Channel type
#define BASS_CTYPE_STREAM_AES67    0x1f200

// Configuration options (use with BASS_SetConfig/BASS_GetConfig)

// General settings
#define BASS_CONFIG_AES67_PT            0x20000  // RTP payload type (default 96)
#define BASS_CONFIG_AES67_INTERFACE     0x20001  // Network interface IP (string ptr)
#define BASS_CONFIG_AES67_JITTER        0x20002  // Jitter buffer depth in ms

// PTP settings
#define BASS_CONFIG_AES67_PTP_DOMAIN    0x20003  // PTP domain (default 0)
#define BASS_CONFIG_AES67_PTP_STATS     0x20004  // PTP stats string (read-only, ptr)
#define BASS_CONFIG_AES67_PTP_OFFSET    0x20005  // PTP offset in nanoseconds (read-only, i64)
#define BASS_CONFIG_AES67_PTP_STATE     0x20006  // PTP state (read-only, see BASS_AES67_PTP_*)
#define BASS_CONFIG_AES67_PTP_ENABLED   0x20007  // Enable/disable PTP (default 1)

// Stream statistics (read-only)
#define BASS_CONFIG_AES67_BUFFER_LEVEL      0x20010  // Buffer fill % (0-200, 100=target)
#define BASS_CONFIG_AES67_JITTER_UNDERRUNS  0x20011  // Jitter buffer underrun count
#define BASS_CONFIG_AES67_PACKETS_RECEIVED  0x20012  // Total packets received
#define BASS_CONFIG_AES67_PACKETS_LATE      0x20013  // Late/dropped packet count
#define BASS_CONFIG_AES67_BUFFER_PACKETS    0x20014  // Current buffer level in packets
#define BASS_CONFIG_AES67_TARGET_PACKETS    0x20015  // Target buffer level in packets
#define BASS_CONFIG_AES67_PACKET_TIME       0x20016  // Detected packet time in microseconds

// PTP/Clock status (read-only)
#define BASS_CONFIG_AES67_PTP_LOCKED    0x20017  // Clock locked status (0=no, 1=yes)
#define BASS_CONFIG_AES67_PTP_FREQ      0x20018  // Clock frequency PPM x 1000 (i32)

// Clock settings
#define BASS_CONFIG_AES67_CLOCK_MODE            0x20019  // Clock mode (see BASS_AES67_CLOCK_*)
#define BASS_CONFIG_AES67_CLOCK_FALLBACK_TIMEOUT 0x2001A // Fallback timeout in seconds (0=disabled, default 5)

// Clock mode values (for BASS_CONFIG_AES67_CLOCK_MODE)
#define BASS_AES67_CLOCK_PTP        0  // IEEE 1588v2 PTP (default)
#define BASS_AES67_CLOCK_LIVEWIRE   1  // Axia Livewire Clock
#define BASS_AES67_CLOCK_SYSTEM     2  // System clock (free-running, no sync)

// Clock state values (for BASS_CONFIG_AES67_PTP_STATE)
#define BASS_AES67_PTP_DISABLED     0  // Clock not running
#define BASS_AES67_PTP_LISTENING    1  // Waiting for master
#define BASS_AES67_PTP_UNCALIBRATED 2  // Syncing with master
#define BASS_AES67_PTP_SLAVE        3  // Locked to master (or fallback active)

// Clock control functions (for output-only mode without input streams)
// Set BASS_CONFIG_AES67_INTERFACE, BASS_CONFIG_AES67_CLOCK_MODE, and
// BASS_CONFIG_AES67_PTP_DOMAIN before calling BASS_AES67_ClockStart()

BOOL BASSDEF(BASS_AES67_ClockStart)();  // Start clock (returns TRUE on success)
BOOL BASSDEF(BASS_AES67_ClockStop)();   // Stop clock (returns TRUE on success)

// =============================================================================
// AES67 OUTPUT STREAM
// =============================================================================

// Output stream configuration (must match Rust Aes67OutputConfigFFI)
typedef struct {
    BYTE multicast_addr[4];   // Multicast IP as bytes (a.b.c.d)
    WORD port;                // UDP port (typically 5004)
    BYTE interface_addr[4];   // Interface IP as bytes (0.0.0.0 for default)
    BYTE payload_type;        // RTP payload type (typically 96)
    WORD channels;            // Number of audio channels
    DWORD sample_rate;        // Sample rate in Hz (typically 48000)
    DWORD packet_time_us;     // Packet time in microseconds (250, 1000, 5000)
} BASS_AES67_OUTPUT_CONFIG;

// Output stream statistics (must match Rust OutputStatsFFI)
typedef struct {
    QWORD packets_sent;       // Total packets transmitted
    QWORD samples_sent;       // Total samples transmitted
    QWORD send_errors;        // Transmission errors
    QWORD underruns;          // Buffer underruns
} BASS_AES67_OUTPUT_STATS;

// Output stream handle (opaque pointer)
typedef void* HAES67OUTPUT;

// Output stream functions
HAES67OUTPUT BASSDEF(BASS_AES67_OutputCreate)(DWORD bass_channel, const BASS_AES67_OUTPUT_CONFIG* config);
BOOL BASSDEF(BASS_AES67_OutputStart)(HAES67OUTPUT handle);
BOOL BASSDEF(BASS_AES67_OutputStop)(HAES67OUTPUT handle);
BOOL BASSDEF(BASS_AES67_OutputGetStats)(HAES67OUTPUT handle, BASS_AES67_OUTPUT_STATS* stats);
BOOL BASSDEF(BASS_AES67_OutputIsRunning)(HAES67OUTPUT handle);
DWORD BASSDEF(BASS_AES67_OutputGetPPM)(HAES67OUTPUT handle);  // Returns PPM x 1000
BOOL BASSDEF(BASS_AES67_OutputFree)(HAES67OUTPUT handle);

#ifdef __cplusplus
}
#endif

#endif // BASS_AES67_H
