// macOS implementation of the audio worker's C ABI. PCM voices share one
// stereo AudioQueue; MIDI uses Apple's General MIDI DLS synthesizer.
#include <AudioToolbox/AudioToolbox.h>
#include <AudioUnit/AudioUnit.h>
#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <limits>
#include <mutex>
#include <new>
#include <utility>
#include <vector>

namespace {
constexpr unsigned rate = 48000;
constexpr unsigned frames = 512;
struct Voice {
    std::vector<float> samples;
    double position = 0, step = 1;
    float gain = 0, pan = 0;
    int id = 0, group = 0;
    unsigned channels = 1;
    bool loop = false;
};
struct Engine {
    AudioQueueRef queue = nullptr;
    std::mutex mutex;
    std::array<Voice, 32> voices;
    int serial = 0;
};

// The callback allocates nothing. Completed buffers are reclaimed/replaced
// by the worker, not by the audio thread.
void render(void* context, AudioQueueRef queue, AudioQueueBufferRef buffer) {
    auto& e = *static_cast<Engine*>(context);
    auto* out = static_cast<float*>(buffer->mAudioData);
    std::fill(out, out + frames * 2, 0.f);
    {
        std::lock_guard<std::mutex> lock(e.mutex);
        for (auto& v : e.voices) {
            if (!v.id) continue;
            const auto count = v.samples.size() / v.channels;
            // Same centibel pan law as the Windows output matrix.
            const float left = v.gain * std::pow(10.f, -std::max(v.pan, 0.f) / 2000.f);
            const float right = v.gain * std::pow(10.f, std::min(v.pan, 0.f) / 2000.f);
            for (unsigned f = 0; f < frames; ++f) {
                if (v.position >= count) {
                    if (v.loop) v.position = std::fmod(v.position, double(count));
                    else { v.id = 0; break; }
                }
                const auto a = size_t(v.position);
                const auto b = a + 1 < count ? a + 1 : (v.loop ? 0 : a);
                const float fraction = float(v.position - a);
                for (unsigned c = 0; c < 2; ++c) {
                    const auto channel = v.channels == 1 ? 0 : c;
                    const auto x = v.samples[a * v.channels + channel];
                    const auto y = v.samples[b * v.channels + channel];
                    out[f * 2 + c] += (x + (y - x) * fraction) * (c ? right : left);
                }
                v.position += v.step;
            }
            if (!v.loop && v.position >= count) v.id = 0;
        }
    }
    for (unsigned i = 0; i < frames * 2; ++i)
        out[i] = std::clamp(out[i], -1.f, 1.f);
    buffer->mAudioDataByteSize = frames * 2 * sizeof(float);
    AudioQueueEnqueueBuffer(queue, buffer, 0, nullptr);
}

struct Midi {
    AUGraph graph = nullptr;
    AudioUnit synth = nullptr;
    ~Midi() {
        if (graph) {
            AUGraphStop(graph);
            DisposeAUGraph(graph);
        }
    }
};
}

extern "C" void tz_audio_destroy(Engine* e) {
    if (!e) return;
    // Wait for callbacks to finish before destroying their context/mutex.
    if (e->queue) AudioQueueDispose(e->queue, true);
    delete e;
}

extern "C" Engine* tz_audio_create(int32_t* error) {
    auto* e = new (std::nothrow) Engine;
    if (!e) { *error = -108; return nullptr; }
    AudioStreamBasicDescription format{};
    format.mSampleRate = rate;
    format.mFormatID = kAudioFormatLinearPCM;
    format.mFormatFlags = kAudioFormatFlagsNativeFloatPacked;
    format.mBytesPerPacket = format.mBytesPerFrame = 2 * sizeof(float);
    format.mFramesPerPacket = 1;
    format.mChannelsPerFrame = 2;
    format.mBitsPerChannel = 32;
    *error = AudioQueueNewOutput(&format, render, e, nullptr, nullptr, 0, &e->queue);
    for (int i = 0; !*error && i < 3; ++i) {
        AudioQueueBufferRef buffer = nullptr;
        *error = AudioQueueAllocateBuffer(e->queue, frames * 2 * sizeof(float), &buffer);
        if (!*error) {
            std::fill_n(static_cast<float*>(buffer->mAudioData), frames * 2, 0.f);
            buffer->mAudioDataByteSize = frames * 2 * sizeof(float);
            *error = AudioQueueEnqueueBuffer(e->queue, buffer, 0, nullptr);
        }
    }
    if (!*error) *error = AudioQueueStart(e->queue, nullptr);
    if (*error) { tz_audio_destroy(e); return nullptr; }
    return e;
}

extern "C" int32_t tz_audio_play(Engine* e, const uint8_t* bytes, unsigned size,
    unsigned sample_rate, uint16_t channels, uint16_t bits, int group,
    int loop, float gain, float pan) {
    if (!bytes || !sample_rate || (channels != 1 && channels != 2) ||
        (bits != 8 && bits != 16) || !size || size % (channels * bits / 8)) return -50;
    Voice v;
    v.channels = channels;
    v.samples.resize(size / (bits / 8));
    for (size_t i = 0; i < v.samples.size(); ++i) {
        if (bits == 8) v.samples[i] = (int(bytes[i]) - 128) / 128.f;
        else {
            const auto word = uint16_t(bytes[i * 2]) | (uint16_t(bytes[i * 2 + 1]) << 8);
            v.samples[i] = (word < 32768 ? int(word) : int(word) - 65536) / 32768.f;
        }
    }
    v.step = double(sample_rate) / rate;
    v.gain = gain;
    v.pan = std::clamp(pan, -10000.f, 10000.f);
    v.group = group;
    v.loop = loop != 0;
    std::unique_lock<std::mutex> lock(e->mutex);
    for (auto& slot : e->voices) {
        if (!slot.id) {
            // Avoid signed overflow and a collision with a live voice.
            do {
                e->serial = e->serial == std::numeric_limits<int>::max() ? 1 : e->serial + 1;
            } while (std::any_of(e->voices.begin(), e->voices.end(),
                [&](const Voice& live) { return live.id == e->serial; }));
            v.id = e->serial;
            std::swap(slot, v);
            const int id = slot.id;
            // Reclaim the previous buffer after releasing the callback's lock.
            lock.unlock();
            return id;
        }
    }
    return -108;
}

extern "C" int tz_audio_voice_active(Engine* e, int id) {
    std::lock_guard<std::mutex> lock(e->mutex);
    return id > 0 && std::any_of(e->voices.begin(), e->voices.end(),
        [id](const Voice& v) { return v.id == id; });
}
extern "C" void tz_audio_stop_voice(Engine* e, int id) {
    std::lock_guard<std::mutex> lock(e->mutex);
    for (auto& v : e->voices) if (v.id == id) v.id = 0;
}
extern "C" void tz_audio_stop(Engine* e, int group) {
    std::lock_guard<std::mutex> lock(e->mutex);
    for (auto& v : e->voices) if (v.group == group) v.id = 0;
}
extern "C" void tz_audio_set(Engine* e, int id, float gain, float pan) {
    std::lock_guard<std::mutex> lock(e->mutex);
    for (auto& v : e->voices) if (v.id == id) {
        v.gain = gain;
        v.pan = std::clamp(pan, -10000.f, 10000.f);
    }
}

// WinMM-compatible argument layout keeps the Rust scheduler shared.
extern "C" uint32_t tz_midi_open(Midi** result, uint32_t, uintptr_t, uintptr_t, uint32_t) {
    *result = nullptr;
    auto* m = new (std::nothrow) Midi;
    if (!m) return uint32_t(-108);
    OSStatus error = NewAUGraph(&m->graph);
    AUNode synth = 0, output = 0;
    AudioComponentDescription instrument{kAudioUnitType_MusicDevice,
        kAudioUnitSubType_DLSSynth, kAudioUnitManufacturer_Apple, 0, 0};
    AudioComponentDescription device{kAudioUnitType_Output,
        kAudioUnitSubType_DefaultOutput, kAudioUnitManufacturer_Apple, 0, 0};
    if (!error) error = AUGraphAddNode(m->graph, &instrument, &synth);
    if (!error) error = AUGraphAddNode(m->graph, &device, &output);
    if (!error) error = AUGraphOpen(m->graph);
    if (!error) error = AUGraphNodeInfo(m->graph, synth, nullptr, &m->synth);
    if (!error) error = AUGraphConnectNodeInput(m->graph, synth, 0, output, 0);
    if (!error) error = AUGraphInitialize(m->graph);
    if (!error) error = AUGraphStart(m->graph);
    if (error) { delete m; return uint32_t(error); }
    *result = m;
    return 0;
}
extern "C" uint32_t tz_midi_message(Midi* m, uint32_t message) {
    return uint32_t(MusicDeviceMIDIEvent(m->synth, message & 255,
        (message >> 8) & 127, (message >> 16) & 127, 0));
}
extern "C" uint32_t tz_midi_reset(Midi* m) {
    OSStatus error = 0;
    for (unsigned ch = 0; ch < 16; ++ch) {
        const auto status = MusicDeviceMIDIEvent(m->synth, 0xb0 | ch, 120, 0, 0);
        if (status) error = status;
    }
    return uint32_t(error);
}
extern "C" uint32_t tz_midi_close(Midi* m) {
    delete m;
    return 0;
}
