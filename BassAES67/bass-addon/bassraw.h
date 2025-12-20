#ifndef BASSRAW_H
#define BASSRAW_H

#include "bass.h"

#if BASSVERSION!=0x204
#error conflicting BASS and BASSRAW versions
#endif

#ifdef __cplusplus
extern "C" {
#endif

#ifndef BASSRAWDEF
#define BASSRAWDEF(f) WINAPI f
#endif

// Additional config options
#define BASS_CONFIG_RAW_FREQ	0x11000
#define BASS_CONFIG_RAW_CHANS	0x11001

// BASS_CHANNELINFO type
#define BASS_CTYPE_STREAM_RAW	0x10000
	
// the above constants should be unique (ask if unsure)


HSTREAM BASSRAWDEF(BASS_RAW_StreamCreateFile)(BOOL mem, const void *file, QWORD offset, QWORD length, DWORD flags);
HSTREAM BASSRAWDEF(BASS_RAW_StreamCreateURL)(const char *url, DWORD offset, DWORD flags, DOWNLOADPROC *proc, void *user);
HSTREAM BASSRAWDEF(BASS_RAW_StreamCreateFileUser)(DWORD system, DWORD flags, const BASS_FILEPROCS *procs, void *user);

#ifdef __cplusplus
}

#ifdef _WIN32
static inline HSTREAM BASS_RAW_StreamCreateFile(BOOL mem, const WCHAR *file, QWORD offset, QWORD length, DWORD flags)
{
	return BASS_RAW_StreamCreateFile(mem, (const void*)file, offset, length, flags | BASS_UNICODE);
}

static inline HSTREAM BASS_RAW_StreamCreateURL(const WCHAR *url, DWORD offset, DWORD flags, DOWNLOADPROC *proc, void *user)
{
	return BASS_RAW_StreamCreateURL((const char*)url, offset, flags | BASS_UNICODE, proc, user);
}
#endif
#endif

#endif
