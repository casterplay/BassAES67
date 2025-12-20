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

// PTP status (read-only)
#define BASS_CONFIG_AES67_PTP_LOCKED    0x20017  // PTP locked status (0=no, 1=yes)
#define BASS_CONFIG_AES67_PTP_FREQ      0x20018  // PTP frequency PPM x 1000 (i32)

// PTP state values (for BASS_CONFIG_AES67_PTP_STATE)
#define BASS_AES67_PTP_DISABLED     0  // PTP not running
#define BASS_AES67_PTP_LISTENING    1  // Waiting for master
#define BASS_AES67_PTP_UNCALIBRATED 2  // Syncing with master
#define BASS_AES67_PTP_SLAVE        3  // Locked to master

#ifdef __cplusplus
}
#endif

#endif // BASS_AES67_H
