// Simple BASS add-on example to play RAW/PCM files

#define VERSION 0x02040000

#include <stdio.h>
#include <malloc.h>
#include <string.h>

#include "bass-addon.h"
const BASS_FUNCTIONS *bassfunc;
const BASS_FUNCTIONS2 *bassfunc2;

#ifndef _MSC_VER
#pragma GCC visibility push(default)
#endif
#include "bassraw.h"
#ifndef _MSC_VER
#pragma GCC visibility pop
#endif

#ifdef _WIN32
#include <tchar.h>
#else
#define _T(x) x
static bool badbass; // incorrect BASS version?
#endif

namespace config {
static DWORD freq = 44100, chans = 2;

// BASSCONFIGPROC called by BASS_SetConfig/Ptr and BASS_GetConfig/Ptr
static BOOL CALLBACK Handler(DWORD option, DWORD flags, void *value)
{
	if (!(flags & BASSCONFIG_PTR)) {
		DWORD *dvalue = (DWORD*)value;
		switch (option) {
			case BASS_CONFIG_RAW_FREQ:
				if (flags & BASSCONFIG_SET) freq = *dvalue;
				else *dvalue = freq;
				return true;

			case BASS_CONFIG_RAW_CHANS:
				if (flags & BASSCONFIG_SET) chans = *dvalue;
				else *dvalue = chans;
				return true;
		}
	}
	return false;
}

}

static const DWORD SYNCTYPEMASK = 0x00ffffff;
static const DWORD SYNCFLAGMASK = 0xff000000;

struct RAWSTREAM {
	HSTREAM handle;
	BASSFILE file;
	DWORD fileoffset;
	QWORD length;

	struct SYNC {
		SYNC	*next;
		HSYNC	handle;
		DWORD	type;
		QWORD	param;
	} *syncs;

	static const ADDON_FUNCTIONS addonfuncs;
	static void WINAPI Free(void *inst);
	static QWORD WINAPI GetLength(void *inst, DWORD mode);
	static const char *WINAPI GetTags(void *inst, DWORD tags);
//	static QWORD WINAPI GetFilePosition(void *inst, DWORD mode);
	static void WINAPI GetInfo(void *inst, BASS_CHANNELINFO *info);
	static BOOL WINAPI CanSetPosition(void *inst, QWORD pos, DWORD mode);
	static QWORD WINAPI SetPosition(void *inst, QWORD pos, DWORD mode);
//	static QWORD WINAPI GetPosition(void *inst, QWORD pos, DWORD mode);
	static HSYNC WINAPI SetSync(void *inst, DWORD type, QWORD param, SYNCPROC *proc, void *user);
	static void WINAPI RemoveSync(void *inst, HSYNC sync);
//	static BOOL WINAPI CanResume(void *inst);
//	static DWORD WINAPI SetFlags(void *inst, DWORD flags);
//	static BOOL WINAPI Attribute(void *inst, DWORD attrib, float *value, BOOL set);
//	static DWORD WINAPI AttributeEx(void *inst, DWORD attrib, void *value, DWORD typesize, BOOL set);
	static DWORD CALLBACK StreamProc(HSTREAM handle, BYTE *buffer, DWORD length, RAWSTREAM *stream);
};

const ADDON_FUNCTIONS RAWSTREAM::addonfuncs = {
	ADDON_LOCK,
	Free,
	GetLength,
	NULL, // have no tags
	NULL, // let BASS handle file position
	GetInfo,
	CanSetPosition,
	SetPosition,
	NULL, // let BASS handle the position/looping/syncing (POS/END)
	SetSync,
	RemoveSync,
	NULL, // let BASS decide when to resume a stalled stream
	NULL, // no custom flags
	NULL, // no attributes
	NULL // no attributes
};

// trigger sync (at current stream position) macro
#define TriggerSync(stream,sync,data)\
	bassfunc->TriggerSync((stream)->handle,(sync)->handle,bassfunc->GetCount((stream)->handle,false),data)

static HSTREAM WINAPI StreamCreateProc(BASSFILE file, DWORD flags)
{
	DWORD fileflags = bassfunc->file.GetFlags(file);

	RAWSTREAM *stream = (RAWSTREAM*)calloc(1, sizeof(RAWSTREAM));
	if (!stream) error(BASS_ERROR_MEM);
	stream->file = file;

	// restrict flags to valid ones, and create the BASS stream
	flags &= BASS_SAMPLE_FLOAT | BASS_SAMPLE_8BITS | BASS_SAMPLE_SOFTWARE | BASS_SAMPLE_LOOP | BASS_SAMPLE_3D | BASS_SAMPLE_FX
		| BASS_STREAM_DECODE | BASS_STREAM_AUTOFREE | 0x3f000000; // 0x3f000000 = all speaker flags
	flags |= fileflags & BASS_STREAM_BLOCK; // BLOCK flag disables seeking
	HSTREAM handle = bassfunc->CreateStream(config::freq, config::chans, flags, (STREAMPROC*)&RAWSTREAM::StreamProc, stream, &RAWSTREAM::addonfuncs);
	if (!handle) { // stream creation failed
		stream->Free(stream);
		return 0; // CreateStream set the error code
	}

	stream->handle = handle;
	stream->length = bassfunc->file.GetPos(file, BASS_FILEPOS_END); // playback length is the file length in this case

	if (fileflags & BASSFILE_BUFFERED) {
		DWORD rate = config::freq * config::chans * (flags & BASS_SAMPLE_FLOAT ? 4 : (flags & BASS_SAMPLE_8BITS ? 1 : 2)); // bytes/sec
		if (!bassfunc->file.StartThread(file, rate, 0)) { // start net/buffered stream download thread
			DWORD err = BASS_ErrorGetCode(); // get error code
			if (!err) err = BASS_ERROR_MEM; // StartThread doesn't set an error code before 2.4.15
			BASS_StreamFree(handle);
			error(err);
		}
	}
	bassfunc->file.SetStream(file, handle); // associate the stream and file (this must be after all error checking)

	if (BASS_GetVersion() >= 0x02041000) BASS_ChannelLock(handle, false); // for ADDON_LOCK
	noerrorn(handle);
}

HSTREAM WINAPI BASS_RAW_StreamCreateFile(BOOL mem, const void *file, QWORD offset, QWORD length, DWORD flags)
{
#ifndef _WIN32
	if (badbass) error(BASS_ERROR_VERSION);
#endif
	BASSFILE bfile = bassfunc->file.Open(mem, file, offset, length, flags, true);
	if (!bfile) return 0; // Open set the error code
	HSTREAM s = StreamCreateProc(bfile, flags);
	if (!s) bassfunc->file.Close(bfile);
	return s;
}

HSTREAM WINAPI BASS_RAW_StreamCreateURL(const char *url, DWORD offset, DWORD flags, DOWNLOADPROC *proc, void *user)
{
#ifndef _WIN32
	if (badbass) error(BASS_ERROR_VERSION);
#endif
	BASSFILE bfile = bassfunc->file.OpenURL(url, offset, flags, proc, user, true);
	if (!bfile) return 0; // OpenURL set the error code
	HSTREAM s = StreamCreateProc(bfile, flags);
	if (!s) bassfunc->file.Close(bfile);
	return s;
}

HSTREAM WINAPI BASS_RAW_StreamCreateFileUser(DWORD system, DWORD flags, const BASS_FILEPROCS *procs, void *user)
{
#ifndef _WIN32
	if (badbass) error(BASS_ERROR_VERSION);
#endif
	BASSFILE bfile = bassfunc->file.OpenUser(system, flags, procs, user, true);
	if (!bfile) return 0; // OpenUser set the error code
	HSTREAM s = StreamCreateProc(bfile, flags);
	if (!s) bassfunc->file.Close(bfile);
	return s;
}

// free the stream's resources
void WINAPI RAWSTREAM::Free(void *inst)
{
	RAWSTREAM *stream = (RAWSTREAM*)inst;
	for (SYNC *s = stream->syncs, *ns; s; s = ns) {
		ns = s->next;
		free(s);
	}
	free(stream);
}

// called by BASS_ChannelGetLength
QWORD WINAPI RAWSTREAM::GetLength(void *inst, DWORD mode)
{
	RAWSTREAM *stream = (RAWSTREAM*)inst;
	if (mode != BASS_POS_BYTE) errorn(BASS_ERROR_NOTAVAIL); // only support byte positioning
	noerrorn(stream->length);
}

// called by BASS_ChannelGetInfo
void WINAPI RAWSTREAM::GetInfo(void *inst, BASS_CHANNELINFO *info)
{
	RAWSTREAM *stream = (RAWSTREAM*)inst;
// set any custom flags and the "ctype" & "origres" values here
	info->ctype = BASS_CTYPE_STREAM_RAW;
}

// called by BASS_ChannelSetPosition
// return TRUE if seeking to the requested position is possible, otherwise return FALSE and set the error code
BOOL WINAPI RAWSTREAM::CanSetPosition(void *inst, QWORD pos, DWORD mode)
{
	RAWSTREAM *stream = (RAWSTREAM*)inst;
	if ((BYTE)mode != BASS_POS_BYTE) error(BASS_ERROR_NOTAVAIL); // only support byte positioning (BYTE = ignore flags)
	if (pos >= stream->length) error(BASS_ERROR_POSITION);
	return true;
}

// called by BASS_ChannelSetPosition after the above function to do the actual seeking
// return the actual resulting position (-1 = error)
QWORD WINAPI RAWSTREAM::SetPosition(void *inst, QWORD pos, DWORD mode)
{
	RAWSTREAM *stream = (RAWSTREAM*)inst;
	if (!bassfunc->file.Seek(stream->file, pos)) errorn(BASS_ERROR_POSITION);
	return pos;
}

// OPTIONAL: called by BASS_ChannelSetSync
// return -1 to let BASS handle the sync (POS/END)
HSYNC WINAPI RAWSTREAM::SetSync(void *inst, DWORD type, QWORD param, SYNCPROC *proc, void *user)
{
	RAWSTREAM *stream = (RAWSTREAM*)inst;

	// check that the sync type is ok
	switch (type & SYNCTYPEMASK) {
		// add supported sync types here

		default:
			return -1; // let BASS handle it
	}

	SYNC *s = (SYNC*)malloc(sizeof(*s));
	if (!s) error(BASS_ERROR_MEM);
	HSYNC sync = bassfunc->NewSync(stream->handle, type, proc, user);
	if (!sync) {
		free(s);
		return 0; // NewSync set the error code
	}
	s->handle = sync;
	s->type = type & SYNCTYPEMASK;
	s->param = param;
	s->next = stream->syncs;
	stream->syncs = s;
	noerrorn(sync);
}

// called when a sync is removed, either by BASS_ChannelRemoveSync or due to ONETIME flag
void WINAPI RAWSTREAM::RemoveSync(void *inst, HSYNC sync)
{
	RAWSTREAM *stream = (RAWSTREAM*)inst;
	for (SYNC **ps = &stream->syncs, *s; s = *ps; ps = &s->next) {
		if (s->handle == sync) {
			*ps = s->next;
			free(s);
			break;
		}
	}
}

/*
// OPTIONAL: called by BASS_ChannelGetTags
const char *WINAPI RAWSTREAM::GetTags(void *inst, DWORD tags)
{
	RAWSTREAM *stream = (RAWSTREAM*)inst;
	switch (tags) {
		// add supported tag types here
	}
	return NULL;
}

// called by BASS_StreamGetFilePosition (OPTIONAL when using BASSFILE routines)
QWORD WINAPI RAWSTREAM::GetFilePosition(void *inst, DWORD mode)
{
	RAWSTREAM *stream = (RAWSTREAM*)inst;
	QWORD r = bassfunc->file.GetPos(stream->file, mode);
	if (r == -1) errorn(BASS_ERROR_NOTAVAIL);
	noerrorn(r);
}

// OPTIONAL: called by BASS_ChannelGetPosition
// pos=byte count since playback was last started
// if this function is omitted, BASS will handle the position
QWORD WINAPI RAWSTREAM::GetPosition(void *inst, QWORD pos, DWORD mode)
{
// translate "pos" to the actual stream position
}

// OPTIONAL: called by BASS_ChannelFlags
// return accepted flags (BASS only uses BASS_SAMPLE_LOOP/BASS_STREAM_AUTOFREE/BASS_STREAM_RESTRATE)
DWORD WINAPI RAWSTREAM::SetFlags(void *inst, DWORD flags)
{
// process any custom flags here
	return flags;
}

// OPTIONAL: called by BASS_ChannelGet/SetAttribute
BOOL WINAPI RAWSTREAM::Attribute(void *inst, DWORD attrib, float *value, BOOL set)
{
	return AttributeEx(inst, attrib, value, BASS_ATTRIBTYPE_FLOAT, set);
}

// OPTIONAL: called by BASS_ChannelGet/SetAttributeEx
DWORD WINAPI RAWSTREAM::AttributeEx(void *inst, DWORD attrib, void *value, DWORD typesize, BOOL set)
{
	RAWSTREAM *stream = (RAWSTREAM*)inst;
	float valuef;
	int valuei;
	if ((int)typesize < 0) { // got a type rather than byte size
		if (typesize != BASS_ATTRIBTYPE_FLOAT && typesize != BASS_ATTRIBTYPE_INT) error(BASS_ERROR_ILLPARAM); // unknown type
		if (set) { // get the float and int value for convenience
			if (typesize == BASS_ATTRIBTYPE_FLOAT)
				valuei = (int)(valuef = *(float*)value);
			else
				valuef = (float)(valuei = *(int*)value);
		}
	}

	switch (attrib) {
		// add supported attributes here
	}
	error(BASS_ERROR_ILLTYPE);
}
*/

/*
// OPTIONAL: called when playback is stalled
// return TRUE to resume playback, eg. after buffering
// if this function is omitted, BASS will decide when to resume
BOOL WINAPI RAWSTREAM::CanResume(void *inst)
{
	RAWSTREAM *stream = (RAWSTREAM*)inst;
	return bassfunc->file.CanResume(stream->file);
}
*/

DWORD CALLBACK RAWSTREAM::StreamProc(HSTREAM handle, BYTE *buffer, DWORD length, RAWSTREAM *stream)
{
	DWORD c = bassfunc->file.Read(stream->file, buffer, length); // read from file
	if (bassfunc->file.Eof(stream->file)) { // it's ended
		stream->length = bassfunc->GetPosition(handle, (QWORD)-1, BASS_POS_BYTE) + c; // update length in case it's different
		c |= BASS_STREAMPROC_END; // set "end" flag
	}
	return c;
}

#ifdef __cplusplus
extern "C"
#endif
#ifndef _WIN32
__attribute__((visibility("default")))
#endif
const void *WINAPI BASSplugin(DWORD face)
{
	static const BASS_PLUGINFORM pluginforms[] = {
		{ BASS_CTYPE_STREAM_RAW, _T("RAW PCM"), _T("*.raw;*.pcm") },
	};
	static const BASS_PLUGININFO plugininfo = { VERSION, 1, pluginforms };

#ifndef _WIN32
	if (badbass) return NULL;
#endif
	switch (face) {
		case BASSPLUGIN_INFO:
			return (void*)&plugininfo;

		case BASSPLUGIN_CREATE:
			return (void*)StreamCreateProc;

/*		case BASSPLUGIN_CREATEURL:
			return (void*)StreamCreateURLProc;*/
	}
	return NULL;
}

#ifdef _WIN32
BOOL WINAPI DllMain(HANDLE hDLL, DWORD reason, LPVOID reserved)
{
	switch (reason) {
		case DLL_PROCESS_ATTACH:
			DisableThreadLibraryCalls((HMODULE)hDLL);
			if (HIWORD(BASS_GetVersion()) != BASSVERSION || !GetBassFunc()) {
#ifdef MessageBox
				MessageBox(0, _T("Incorrect BASS.DLL version (")_T(BASSVERSIONTEXT)_T(" is required)"), _T("BASS_RAW"), MB_ICONERROR);
#endif
				return false;
			}
			GetBassFunc2();
			bassfunc->RegisterPlugin((void*)config::Handler, PLUGIN_CONFIG_ADD); // register config function for freq/chans options
			break;

		case DLL_PROCESS_DETACH:
			if (!reserved)
				bassfunc->RegisterPlugin((void*)config::Handler, PLUGIN_CONFIG_REMOVE); // unregister the config function
			break;
	}

	return true;
}
#else
#include <stdio.h>
static void __attribute__((constructor)) PROCESS_ATTACH()
{
	badbass = (HIWORD(BASS_GetVersion()) != BASSVERSION) | !GetBassFunc();
#ifdef __ANDROID__
	badbass |= !GetJniFunc();
#endif
	if (badbass)
		fputs("BASS_RAW: Incorrect BASS version (" BASSVERSIONTEXT " is required)", stderr);
	else {
		GetBassFunc2();
		bassfunc->RegisterPlugin((void*)config::Handler, PLUGIN_CONFIG_ADD); // register config function for freq/chans options
	}
}
static void __attribute__((destructor)) PROCESS_DETACH()
{
	if (!badbass)
		bassfunc->RegisterPlugin((void*)config::Handler, PLUGIN_CONFIG_REMOVE); // unregister the config function
}
#endif

#ifdef __ANDROID__
const BASSJNI_FUNCTIONS *jnifunc;

#define JFUNC(func, ...) Java_com_un4seen_bass_BASSRAW_BASS_1RAW_1##func(JNIEnv* env, jobject thiz, ##__VA_ARGS__)

extern "C" {

#pragma GCC visibility push(default)

jint JFUNC(StreamCreateFile, jstring file, jlong offset, jlong length, jint flags)
{
	return BASS_RAW_StreamCreateFile(BASSFILE_MEM_JAVA, file, offset, length, flags);
}

jint JFUNC(StreamCreateURL, jstring url, jint offset, jint flags, jobject proc, jobject user)
{
	const char *utf8 = env->GetStringUTFChars(url, NULL);
	void *p = 0;
	DOWNLOADPROC *nproc = 0;
	if (proc) {
		p = jnifunc->callback.NewDownloadProc(env, proc, user, &nproc);
		if (!p) return 0;
	}
	HSTREAM r = BASS_RAW_StreamCreateURL(utf8, offset, flags & ~BASS_UNICODE, nproc, p);
	if (p) {
		if (r) jnifunc->callback.SetFreeSync(env, r, p);
		else jnifunc->callback.Free(p);
	}
	env->ReleaseStringUTFChars(url, utf8);
	return r;
}

jint JFUNC(StreamCreateFileUser, jint system, jint flags, jobject procs, jobject user)
{
	const BASS_FILEPROCS *nprocs;
	void *p = jnifunc->callback.NewFileProcs(env, procs, user, &nprocs);
	if (!p) return 0;
	HSTREAM r = BASS_RAW_StreamCreateFileUser(system, flags, nprocs, p);
	if (r) jnifunc->callback.SetFreeSync(env, r, p);
	else jnifunc->callback.Free(p);
	return r;
}

}
#endif
