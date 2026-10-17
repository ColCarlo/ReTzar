// Small C ABI around Windows 11 XAudio2. Buffer ownership stays in this module;
// which voice to give up when the channels are full is decided by the caller.
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <xaudio2.h>
#include <math.h>
#include <stdlib.h>
#include <string.h>
struct Slot { IXAudio2SourceVoice* voice; void* bytes; int group; long id; unsigned short channels; };
struct Engine { HMODULE dll; IXAudio2* xa; IXAudio2MasteringVoice* master; UINT32 outputs; bool com; long serial; Slot slots[32]; };
static void clear(Slot& s) {
    if (s.voice) s.voice->DestroyVoice();
    free(s.bytes); memset(&s, 0, sizeof(s));
}
extern "C" void tz_audio_destroy(Engine* e) {
    if (!e) return;
    for (auto& s : e->slots) clear(s);
    if (e->master) e->master->DestroyVoice();
    if (e->xa) e->xa->Release();
    if (e->dll) FreeLibrary(e->dll);
    if (e->com) CoUninitialize();
    free(e);
}
extern "C" Engine* tz_audio_create(long* error) {
    Engine* e = (Engine*)calloc(1, sizeof(Engine));
    if (!e) { *error = E_OUTOFMEMORY; return nullptr; }
    HRESULT hr = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    e->com = SUCCEEDED(hr);
    if (FAILED(hr) && hr != RPC_E_CHANGED_MODE) { *error = hr; tz_audio_destroy(e); return nullptr; }
    e->dll = LoadLibraryExW(L"xaudio2_9.dll", nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!e->dll) { *error = HRESULT_FROM_WIN32(GetLastError()); tz_audio_destroy(e); return nullptr; }
    using Create = HRESULT(WINAPI*)(IXAudio2**, UINT32, XAUDIO2_PROCESSOR);
    auto create = (Create)GetProcAddress(e->dll, "XAudio2Create");
    hr = create ? create(&e->xa, 0, XAUDIO2_DEFAULT_PROCESSOR) : E_NOINTERFACE;
    if (SUCCEEDED(hr)) hr = e->xa->CreateMasteringVoice(&e->master);
    *error = hr;
    if (FAILED(hr)) { tz_audio_destroy(e); return nullptr; }
    XAUDIO2_VOICE_DETAILS details{};
    e->master->GetVoiceDetails(&details);
    e->outputs = details.InputChannels;
    return e;
}
extern "C" unsigned tz_audio_poll(Engine* e) {
    unsigned active = 0;
    for (auto& s : e->slots) if (s.voice) {
        XAUDIO2_VOICE_STATE state{};
        s.voice->GetState(&state, XAUDIO2_VOICE_NOSAMPLESPLAYED);
        if (!state.BuffersQueued) clear(s); else ++active;
    }
    return active;
}
extern "C" unsigned tz_audio_group_active(Engine* e, int group) {
    tz_audio_poll(e);
    unsigned count = 0;
    for (auto& s : e->slots) if (s.voice && s.group == group) ++count;
    return count;
}
extern "C" int tz_audio_voice_active(Engine* e, long id) {
    for (auto& s : e->slots) if (s.voice && s.id == id) {
        XAUDIO2_VOICE_STATE state{};
        s.voice->GetState(&state, XAUDIO2_VOICE_NOSAMPLESPLAYED);
        if (state.BuffersQueued) return 1;
        clear(s);
    }
    return 0;
}
extern "C" void tz_audio_stop_voice(Engine* e, long id) {
    for (auto& s : e->slots) if (s.voice && s.id == id) clear(s);
}
extern "C" void tz_audio_stop(Engine* e, int group) {
    for (auto& s : e->slots) if (s.voice && s.group == group) clear(s);
}
extern "C" void tz_audio_gain(Engine* e, int group, float gain) {
    for (auto& s : e->slots) if (s.voice && s.group == group) s.voice->SetVolume(gain);
}
// `pan` follows DirectSound: hundredths of a decibel taken from the opposite
// channel, positive towards the right.
static HRESULT pan_voice(Engine* e, IXAudio2SourceVoice* v, unsigned short channels, float pan, bool force) {
    if ((pan == 0 && !force) || e->outputs < 2 || channels > 2) return S_OK;
    float left = pan > 0 ? powf(10.f, -pan / 2000.f) : 1.f;
    float right = pan < 0 ? powf(10.f, pan / 2000.f) : 1.f;
    float matrix[2 * XAUDIO2_MAX_AUDIO_CHANNELS] = {};
    // Rows are output channels, columns source channels.
    if (channels == 1) { matrix[0] = left; matrix[1] = right; }
    else { matrix[0] = left; matrix[3] = right; }
    return v->SetOutputMatrix(nullptr, channels, e->outputs, matrix);
}
// Returns the voice id (> 0) or a failed HRESULT.
extern "C" long tz_audio_play(Engine* e, const unsigned char* bytes, unsigned size,
                            unsigned rate, unsigned short channels, unsigned short bits,
                            int group, int loop, float gain, float pan) {
    tz_audio_poll(e);
    Slot* slot = nullptr;
    for (auto& s : e->slots) if (!s.voice) { slot = &s; break; }
    if (!slot) return E_OUTOFMEMORY;
    WAVEFORMATEX fmt{};
    fmt.wFormatTag = WAVE_FORMAT_PCM; fmt.nChannels = channels;
    fmt.nSamplesPerSec = rate; fmt.wBitsPerSample = bits;
    fmt.nBlockAlign = channels * bits / 8; fmt.nAvgBytesPerSec = rate * fmt.nBlockAlign;
    HRESULT hr = e->xa->CreateSourceVoice(&slot->voice, &fmt);
    if (FAILED(hr)) return hr;
    slot->bytes = malloc(size); slot->group = group; slot->channels = channels;
    if (!slot->bytes) { clear(*slot); return E_OUTOFMEMORY; }
    memcpy(slot->bytes, bytes, size);
    XAUDIO2_BUFFER buffer{};
    buffer.Flags = XAUDIO2_END_OF_STREAM; buffer.AudioBytes = size;
    buffer.pAudioData = (const BYTE*)slot->bytes;
    buffer.LoopCount = loop ? XAUDIO2_LOOP_INFINITE : 0;
    hr = slot->voice->SubmitSourceBuffer(&buffer);
    if (SUCCEEDED(hr)) hr = slot->voice->SetVolume(gain);
    if (SUCCEEDED(hr)) hr = pan_voice(e, slot->voice, channels, pan, false);
    if (SUCCEEDED(hr)) hr = slot->voice->Start();
    if (FAILED(hr)) { clear(*slot); return hr; }
    if (++e->serial <= 0) e->serial = 1;
    slot->id = e->serial;
    return slot->id;
}
// A playing voice heard from elsewhere: new gain and pan (0x4d01c0).
extern "C" void tz_audio_set(Engine* e, long id, float gain, float pan) {
    for (auto& s : e->slots) if (s.voice && s.id == id) {
        s.voice->SetVolume(gain);
        pan_voice(e, s.voice, s.channels, pan, true);
    }
}
