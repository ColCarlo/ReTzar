// Media Foundation bridge: movies read from memory (MP4 of the portable copy,
// the Italian launcher's MPEG) as RGBA frames and 16-bit PCM, and MP4
// (H.264 and AAC) written by the release builder. Each object belongs to the
// thread that made it.
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <mfapi.h>
#include <mfidl.h>
#include <mfreadwrite.h>
#include <mferror.h>
#include <shlwapi.h>
#include <stdlib.h>
#include <string.h>
// Placement construction only: no C++ library is linked.
#ifndef __PLACEMENT_NEW_INLINE
#define __PLACEMENT_NEW_INLINE
inline void* operator new(size_t, void* memory) noexcept { return memory; }
inline void operator delete(void*, void*) noexcept {}
#endif

template <class T> static void release(T*& p) { if (p) { p->Release(); p = nullptr; } }

// The H.264 encoder's settings of codecapi.h: rate control (3, constant
// quality), quality (0-100) and quality against speed (100, the best).
static const GUID RateControlMode = {0x1c0608e9, 0x370c, 0x4710, {0x8a, 0x58, 0xcb, 0x61, 0x81, 0xc4, 0x24, 0x23}};
static const GUID Quality = {0xfcbf57a3, 0x7ea5, 0x4b0c, {0x96, 0x44, 0x69, 0xb4, 0x0c, 0x39, 0xc3, 0x91}};
static const GUID QualityVsSpeed = {0x98332df8, 0x03cd, 0x476b, {0x89, 0xfa, 0x3f, 0x9e, 0x44, 0x2d, 0xec, 0x9f}};

struct Started {
    bool com = false, mf = false;
    HRESULT start() {
        HRESULT hr = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
        com = SUCCEEDED(hr);
        if (FAILED(hr) && hr != RPC_E_CHANGED_MODE) return hr;
        hr = MFStartup(MF_VERSION, MFSTARTUP_LITE);
        mf = SUCCEEDED(hr);
        return hr;
    }
    void stop() {
        if (mf) MFShutdown();
        if (com) CoUninitialize();
        mf = com = false;
    }
};

// A source reader over a private copy of the bytes, one stream selected.
static HRESULT reader(const BYTE* bytes, UINT len, const wchar_t* type, bool video,
                      IMFSourceReader** out) {
    IStream* stream = SHCreateMemStream(bytes, len);
    if (!stream) return E_OUTOFMEMORY;
    IMFByteStream* bs = nullptr;
    IMFAttributes* hint = nullptr;
    IMFAttributes* options = nullptr;
    HRESULT hr = MFCreateMFByteStreamOnStream(stream, &bs);
    if (SUCCEEDED(hr) && type && *type && SUCCEEDED(bs->QueryInterface(IID_PPV_ARGS(&hint))))
        hr = hint->SetString(MF_BYTESTREAM_CONTENT_TYPE, type);
    if (SUCCEEDED(hr)) hr = MFCreateAttributes(&options, 1);
    if (SUCCEEDED(hr) && video)
        hr = options->SetUINT32(MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, TRUE);
    if (SUCCEEDED(hr)) hr = MFCreateSourceReaderFromByteStream(bs, options, out);
    release(options); release(hint); release(bs); release(stream);
    if (FAILED(hr)) return hr;
    DWORD which = video ? (DWORD)MF_SOURCE_READER_FIRST_VIDEO_STREAM
                        : (DWORD)MF_SOURCE_READER_FIRST_AUDIO_STREAM;
    hr = (*out)->SetStreamSelection((DWORD)MF_SOURCE_READER_ALL_STREAMS, FALSE);
    if (SUCCEEDED(hr)) hr = (*out)->SetStreamSelection(which, TRUE);
    IMFMediaType* wanted = nullptr;
    if (SUCCEEDED(hr)) hr = MFCreateMediaType(&wanted);
    if (SUCCEEDED(hr)) hr = wanted->SetGUID(MF_MT_MAJOR_TYPE, video ? MFMediaType_Video : MFMediaType_Audio);
    if (SUCCEEDED(hr)) hr = wanted->SetGUID(MF_MT_SUBTYPE, video ? MFVideoFormat_RGB32 : MFAudioFormat_PCM);
    if (SUCCEEDED(hr) && !video) hr = wanted->SetUINT32(MF_MT_AUDIO_BITS_PER_SAMPLE, 16);
    if (SUCCEEDED(hr)) hr = (*out)->SetCurrentMediaType(which, nullptr, wanted);
    release(wanted);
    if (FAILED(hr)) release(*out);
    return hr;
}

struct Media {
    Started started;
    IMFSourceReader* video = nullptr;
    // The picture inside the decoded frame (the display aperture).
    UINT32 width = 0, height = 0, left = 0, top = 0, coded_width = 0, coded_height = 0;
    LONG stride = 0;
    short* pcm = nullptr;
    size_t samples = 0, capacity = 0;
    UINT32 rate = 0, channels = 0;
    double seconds = 0;
};

static HRESULT frame_layout(Media* m) {
    IMFMediaType* type = nullptr;
    HRESULT hr = m->video->GetCurrentMediaType((DWORD)MF_SOURCE_READER_FIRST_VIDEO_STREAM, &type);
    if (SUCCEEDED(hr)) hr = MFGetAttributeSize(type, MF_MT_FRAME_SIZE, &m->coded_width, &m->coded_height);
    if (SUCCEEDED(hr)) {
        m->width = m->coded_width; m->height = m->coded_height; m->left = m->top = 0;
        MFVideoArea area{};
        UINT32 size = 0;
        if (SUCCEEDED(type->GetBlob(MF_MT_MINIMUM_DISPLAY_APERTURE, (UINT8*)&area, sizeof(area), &size))
            && size == sizeof(area) && area.Area.cx > 0 && area.Area.cy > 0) {
            m->left = (UINT32)area.OffsetX.value; m->top = (UINT32)area.OffsetY.value;
            m->width = (UINT32)area.Area.cx; m->height = (UINT32)area.Area.cy;
            if (m->left + m->width > m->coded_width || m->top + m->height > m->coded_height) {
                m->width = m->coded_width; m->height = m->coded_height; m->left = m->top = 0;
            }
        }
        UINT32 stride = 0;
        if (SUCCEEDED(type->GetUINT32(MF_MT_DEFAULT_STRIDE, &stride))) m->stride = (LONG)stride;
        else m->stride = (LONG)m->coded_width * 4;
        if (m->coded_width == 0 || m->coded_height == 0 || m->coded_width > 8192 || m->coded_height > 8192)
            hr = MF_E_INVALIDMEDIATYPE;
    }
    release(type);
    return hr;
}

// Room for `count` more samples.
static HRESULT grow(Media* m, size_t count) {
    if (m->samples + count <= m->capacity) return S_OK;
    size_t grown = (m->samples + count) * 2;
    short* bigger = (short*)realloc(m->pcm, grown * sizeof(short));
    if (!bigger) return E_OUTOFMEMORY;
    m->pcm = bigger;
    m->capacity = grown;
    return S_OK;
}

extern "C" void tz_media_close(Media* m) {
    if (!m) return;
    release(m->video);
    free(m->pcm);
    m->started.stop();
    m->~Media();
    free(m);
}

// Opens a movie held in memory. `type` is a MIME type that helps choose the
// source ("video/mp4", "video/mpeg"); audio is decoded at once, frames one
// by one with tz_media_frame.
extern "C" Media* tz_media_open(const BYTE* bytes, size_t len, const wchar_t* type, long* error) {
    void* memory = calloc(1, sizeof(Media));
    if (!memory) { *error = E_OUTOFMEMORY; return nullptr; }
    Media* m = new (memory) Media{};
    HRESULT hr = len > 0x7fffffff ? E_INVALIDARG : m->started.start();
    // The pictures, if there are any (a sound file has none).
    HRESULT pictures = E_FAIL;
    if (SUCCEEDED(hr)) {
        pictures = reader(bytes, (UINT)len, type, true, &m->video);
        if (SUCCEEDED(pictures)) pictures = frame_layout(m);
        if (FAILED(pictures)) release(m->video);
    }
    // The sound, if there is any, decoded whole.
    IMFSourceReader* audio = nullptr;
    if (SUCCEEDED(hr) && SUCCEEDED(reader(bytes, (UINT)len, type, false, &audio))) {
        IMFMediaType* actual = nullptr;
        if (SUCCEEDED(audio->GetCurrentMediaType((DWORD)MF_SOURCE_READER_FIRST_AUDIO_STREAM, &actual))) {
            actual->GetUINT32(MF_MT_AUDIO_SAMPLES_PER_SECOND, &m->rate);
            actual->GetUINT32(MF_MT_AUDIO_NUM_CHANNELS, &m->channels);
            release(actual);
        }
        bool first = true;
        while (SUCCEEDED(hr) && m->rate && m->channels) {
            DWORD flags = 0;
            LONGLONG stamp = 0;
            IMFSample* sample = nullptr;
            HRESULT read = audio->ReadSample((DWORD)MF_SOURCE_READER_FIRST_AUDIO_STREAM, 0, nullptr,
                                             &flags, &stamp, &sample);
            if (FAILED(read)) break;
            if (sample && first) {
                // Sound that starts after the movie's zero (the MPEG intro's
                // streams start late) waits as silence, in step with the
                // pictures' times.
                first = false;
                size_t lead = stamp > 0 ? (size_t)(stamp * m->rate / 10000000) * m->channels : 0;
                if (lead && SUCCEEDED(hr = grow(m, lead))) {
                    memset(m->pcm + m->samples, 0, lead * sizeof(short));
                    m->samples += lead;
                }
            }
            if (sample) {
                IMFMediaBuffer* buffer = nullptr;
                BYTE* data = nullptr;
                DWORD length = 0;
                if (SUCCEEDED(sample->ConvertToContiguousBuffer(&buffer))
                    && SUCCEEDED(buffer->Lock(&data, nullptr, &length))) {
                    size_t count = length / 2;
                    if (SUCCEEDED(hr)) hr = grow(m, count);
                    if (SUCCEEDED(hr)) {
                        memcpy(m->pcm + m->samples, data, count * 2);
                        m->samples += count;
                    }
                    buffer->Unlock();
                }
                release(buffer);
                release(sample);
            }
            if (flags & MF_SOURCE_READERF_ENDOFSTREAM) break;
        }
    }
    IMFSourceReader* any = m->video ? m->video : audio;
    if (SUCCEEDED(hr) && any) {
        PROPVARIANT duration;
        PropVariantInit(&duration);
        if (SUCCEEDED(any->GetPresentationAttribute((DWORD)MF_SOURCE_READER_MEDIASOURCE,
                                                    MF_PD_DURATION, &duration))
            && duration.vt == VT_UI8)
            m->seconds = duration.uhVal.QuadPart / 1e7;
        PropVariantClear(&duration);
    }
    release(audio);
    if (SUCCEEDED(hr) && !m->video && m->samples == 0) hr = pictures;
    *error = hr;
    if (FAILED(hr)) { tz_media_close(m); return nullptr; }
    return m;
}

extern "C" void tz_media_info(Media* m, unsigned* width, unsigned* height, double* seconds,
                              unsigned* rate, unsigned* channels, size_t* samples) {
    *width = m->width; *height = m->height; *seconds = m->seconds;
    *rate = m->rate; *channels = m->channels; *samples = m->samples;
}
extern "C" void tz_media_pcm(Media* m, short* out) {
    if (m->samples) memcpy(out, m->pcm, m->samples * 2);
}

// The next frame as RGBA (width x height x 4), its time in seconds.
// 1: a frame, 0: the end, a negative HRESULT: an error. A frame of another
// size than the one announced ends the movie with MF_E_INVALIDMEDIATYPE.
extern "C" long tz_media_frame(Media* m, BYTE* rgba, double* time) {
    if (!m->video) return 0;
    for (;;) {
        DWORD flags = 0;
        LONGLONG stamp = 0;
        IMFSample* sample = nullptr;
        HRESULT hr = m->video->ReadSample((DWORD)MF_SOURCE_READER_FIRST_VIDEO_STREAM, 0, nullptr,
                                          &flags, &stamp, &sample);
        if (FAILED(hr)) return hr;
        if (flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED) {
            UINT32 w = m->width, h = m->height;
            hr = frame_layout(m);
            if (SUCCEEDED(hr) && (w != m->width || h != m->height)) hr = MF_E_INVALIDMEDIATYPE;
            if (FAILED(hr)) { release(sample); return hr; }
        }
        if (!sample) {
            if (flags & MF_SOURCE_READERF_ENDOFSTREAM) return 0;
            continue;
        }
        IMFMediaBuffer* buffer = nullptr;
        hr = sample->ConvertToContiguousBuffer(&buffer);
        IMF2DBuffer* flat = nullptr;
        BYTE* first = nullptr;
        LONG pitch = 0;
        BYTE* data = nullptr;
        DWORD length = 0;
        bool two_d = SUCCEEDED(hr) && SUCCEEDED(buffer->QueryInterface(IID_PPV_ARGS(&flat)))
                     && SUCCEEDED(flat->Lock2D(&first, &pitch));
        if (!two_d && SUCCEEDED(hr)) {
            hr = buffer->Lock(&data, nullptr, &length);
            if (SUCCEEDED(hr)) {
                pitch = m->stride;
                first = pitch < 0 ? data + (size_t)(-pitch) * (m->coded_height - 1) : data;
                if ((size_t)(pitch < 0 ? -pitch : pitch) * m->coded_height > length) hr = E_UNEXPECTED;
            }
        }
        if (SUCCEEDED(hr)) {
            for (UINT32 y = 0; y < m->height; ++y) {
                const BYTE* row = first + (LONG_PTR)(y + m->top) * pitch + (size_t)m->left * 4;
                BYTE* out = rgba + (size_t)y * m->width * 4;
                for (UINT32 x = 0; x < m->width; ++x) {
                    out[x * 4] = row[x * 4 + 2];
                    out[x * 4 + 1] = row[x * 4 + 1];
                    out[x * 4 + 2] = row[x * 4];
                    out[x * 4 + 3] = 255;
                }
            }
            *time = stamp / 1e7;
        }
        if (two_d) flat->Unlock2D();
        else if (data) buffer->Unlock();
        release(flat);
        release(buffer);
        release(sample);
        return FAILED(hr) ? hr : 1;
    }
}

struct Writer {
    Started started;
    IMFSinkWriter* sink = nullptr;
    DWORD video = 0, audio = 0;
    UINT32 width = 0, height = 0, channels = 0;
    bool has_audio = false;
};

extern "C" void tz_mp4_abandon(Writer* w) {
    if (!w) return;
    release(w->sink);
    w->started.stop();
    w->~Writer();
    free(w);
}

// An MP4 file: H.264 video of constant `quality` (1-100; 0 for a constant
// `bitrate` in bits per second), and AAC sound at 44.1 or 48 kHz when
// `rate` is not zero.
extern "C" Writer* tz_mp4_create(const wchar_t* path, unsigned width, unsigned height,
                                 unsigned fps_num, unsigned fps_den, unsigned bitrate,
                                 unsigned quality, unsigned rate, unsigned channels, long* error) {
    void* memory = calloc(1, sizeof(Writer));
    if (!memory) { *error = E_OUTOFMEMORY; return nullptr; }
    Writer* w = new (memory) Writer{};
    w->width = width; w->height = height; w->channels = channels;
    w->has_audio = rate != 0;
    HRESULT hr = w->started.start();
    IMFAttributes* options = nullptr;
    if (SUCCEEDED(hr)) hr = MFCreateAttributes(&options, 2);
    if (SUCCEEDED(hr)) hr = options->SetGUID(MF_TRANSCODE_CONTAINERTYPE, MFTranscodeContainerType_MPEG4);
    if (SUCCEEDED(hr)) hr = options->SetUINT32(MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, FALSE);
    if (SUCCEEDED(hr)) hr = MFCreateSinkWriterFromURL(path, nullptr, options, &w->sink);
    release(options);
    IMFMediaType *out = nullptr, *in = nullptr;
    if (SUCCEEDED(hr)) hr = MFCreateMediaType(&out);
    if (SUCCEEDED(hr)) hr = out->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Video);
    if (SUCCEEDED(hr)) hr = out->SetGUID(MF_MT_SUBTYPE, MFVideoFormat_H264);
    if (SUCCEEDED(hr)) hr = out->SetUINT32(MF_MT_AVG_BITRATE, bitrate);
    if (SUCCEEDED(hr)) hr = out->SetUINT32(MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive);
    if (SUCCEEDED(hr)) hr = out->SetUINT32(MF_MT_MPEG2_PROFILE, 100 /* eAVEncH264VProfile_High */);
    if (SUCCEEDED(hr)) hr = MFSetAttributeSize(out, MF_MT_FRAME_SIZE, width, height);
    if (SUCCEEDED(hr)) hr = MFSetAttributeRatio(out, MF_MT_FRAME_RATE, fps_num, fps_den);
    if (SUCCEEDED(hr)) hr = MFSetAttributeRatio(out, MF_MT_PIXEL_ASPECT_RATIO, 1, 1);
    if (SUCCEEDED(hr)) hr = w->sink->AddStream(out, &w->video);
    if (SUCCEEDED(hr)) hr = MFCreateMediaType(&in);
    if (SUCCEEDED(hr)) hr = in->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Video);
    if (SUCCEEDED(hr)) hr = in->SetGUID(MF_MT_SUBTYPE, MFVideoFormat_RGB32);
    if (SUCCEEDED(hr)) hr = in->SetUINT32(MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive);
    if (SUCCEEDED(hr)) hr = in->SetUINT32(MF_MT_DEFAULT_STRIDE, width * 4);
    if (SUCCEEDED(hr)) hr = MFSetAttributeSize(in, MF_MT_FRAME_SIZE, width, height);
    if (SUCCEEDED(hr)) hr = MFSetAttributeRatio(in, MF_MT_FRAME_RATE, fps_num, fps_den);
    if (SUCCEEDED(hr)) hr = MFSetAttributeRatio(in, MF_MT_PIXEL_ASPECT_RATIO, 1, 1);
    IMFAttributes* encoding = nullptr;
    if (SUCCEEDED(hr) && quality) {
        hr = MFCreateAttributes(&encoding, 3);
        if (SUCCEEDED(hr)) hr = encoding->SetUINT32(RateControlMode, 3);
        if (SUCCEEDED(hr)) hr = encoding->SetUINT32(Quality, quality);
        if (SUCCEEDED(hr)) hr = encoding->SetUINT32(QualityVsSpeed, 100);
    }
    if (SUCCEEDED(hr)) hr = w->sink->SetInputMediaType(w->video, in, encoding);
    release(encoding);
    release(out); release(in);
    if (SUCCEEDED(hr) && w->has_audio) {
        hr = MFCreateMediaType(&out);
        if (SUCCEEDED(hr)) hr = out->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Audio);
        if (SUCCEEDED(hr)) hr = out->SetGUID(MF_MT_SUBTYPE, MFAudioFormat_AAC);
        if (SUCCEEDED(hr)) hr = out->SetUINT32(MF_MT_AUDIO_BITS_PER_SAMPLE, 16);
        if (SUCCEEDED(hr)) hr = out->SetUINT32(MF_MT_AUDIO_SAMPLES_PER_SECOND, rate);
        if (SUCCEEDED(hr)) hr = out->SetUINT32(MF_MT_AUDIO_NUM_CHANNELS, channels);
        // 192 kbit/s, the highest the encoder takes.
        if (SUCCEEDED(hr)) hr = out->SetUINT32(MF_MT_AUDIO_AVG_BYTES_PER_SECOND, 24000);
        if (SUCCEEDED(hr)) hr = w->sink->AddStream(out, &w->audio);
        if (SUCCEEDED(hr)) hr = MFCreateMediaType(&in);
        if (SUCCEEDED(hr)) hr = in->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Audio);
        if (SUCCEEDED(hr)) hr = in->SetGUID(MF_MT_SUBTYPE, MFAudioFormat_PCM);
        if (SUCCEEDED(hr)) hr = in->SetUINT32(MF_MT_AUDIO_BITS_PER_SAMPLE, 16);
        if (SUCCEEDED(hr)) hr = in->SetUINT32(MF_MT_AUDIO_SAMPLES_PER_SECOND, rate);
        if (SUCCEEDED(hr)) hr = in->SetUINT32(MF_MT_AUDIO_NUM_CHANNELS, channels);
        if (SUCCEEDED(hr)) hr = in->SetUINT32(MF_MT_AUDIO_BLOCK_ALIGNMENT, channels * 2);
        if (SUCCEEDED(hr)) hr = in->SetUINT32(MF_MT_AUDIO_AVG_BYTES_PER_SECOND, rate * channels * 2);
        if (SUCCEEDED(hr)) hr = w->sink->SetInputMediaType(w->audio, in, nullptr);
        release(out); release(in);
    }
    if (SUCCEEDED(hr)) hr = w->sink->BeginWriting();
    *error = hr;
    if (FAILED(hr)) { tz_mp4_abandon(w); return nullptr; }
    return w;
}

static HRESULT write(Writer* w, DWORD stream, const BYTE* bytes, DWORD len, LONGLONG time,
                     LONGLONG duration, bool rgba) {
    IMFMediaBuffer* buffer = nullptr;
    IMFSample* sample = nullptr;
    BYTE* data = nullptr;
    HRESULT hr = MFCreateMemoryBuffer(len, &buffer);
    if (SUCCEEDED(hr)) hr = buffer->Lock(&data, nullptr, nullptr);
    if (SUCCEEDED(hr)) {
        if (rgba) {
            // RGBA in, BGRA (RGB32) out.
            for (DWORD at = 0; at < len; at += 4) {
                data[at] = bytes[at + 2]; data[at + 1] = bytes[at + 1];
                data[at + 2] = bytes[at]; data[at + 3] = 255;
            }
        } else {
            memcpy(data, bytes, len);
        }
        buffer->Unlock();
        hr = buffer->SetCurrentLength(len);
    }
    if (SUCCEEDED(hr)) hr = MFCreateSample(&sample);
    if (SUCCEEDED(hr)) hr = sample->AddBuffer(buffer);
    if (SUCCEEDED(hr)) hr = sample->SetSampleTime(time);
    if (SUCCEEDED(hr)) hr = sample->SetSampleDuration(duration);
    if (SUCCEEDED(hr)) hr = w->sink->WriteSample(stream, sample);
    release(sample);
    release(buffer);
    return hr;
}

// One frame, RGBA top row first, from `time` for `duration` (100 ns units).
extern "C" long tz_mp4_video(Writer* w, const BYTE* rgba, long long time, long long duration) {
    return write(w, w->video, rgba, w->width * w->height * 4, time, duration, true);
}
// Interleaved 16-bit samples from `time` (100 ns units).
extern "C" long tz_mp4_audio(Writer* w, const short* pcm, unsigned frames, long long time,
                             long long duration) {
    if (!w->has_audio) return E_UNEXPECTED;
    return write(w, w->audio, (const BYTE*)pcm, frames * w->channels * 2, time, duration, false);
}
// Closes the file; the writer is gone either way.
extern "C" long tz_mp4_finish(Writer* w) {
    HRESULT hr = w->sink->Finalize();
    tz_mp4_abandon(w);
    return hr;
}
