// AVFoundation implementation of the media bridge. Each handle belongs to
// its calling Rust decoder/encoder thread. No player UI or external executable.
#import <AVFoundation/AVFoundation.h>
#import <AudioToolbox/AudioToolbox.h>
#import <CoreVideo/CoreVideo.h>
#include <algorithm>
#include <atomic>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <exception>
#include <memory>
#include <thread>
#include <vector>

namespace {
NSString* text(const uint16_t* s) {
    size_t size = 0;
    while (s[size]) ++size;
    return [[NSString alloc] initWithCharacters:s length:size];
}
int32_t failed(NSError* error) {
    if (error) NSLog(@"ReTzar media: %@", error);
    return -1;
}
AVAssetTrack* track(AVAsset* asset, AVMediaType type, bool& loaded) {
    if (@available(macOS 12.0, *)) {
        __block NSArray<AVAssetTrack*>* result = nil;
        __block NSError* problem = nil;
        dispatch_semaphore_t done = dispatch_semaphore_create(0);
        [asset loadTracksWithMediaType:type completionHandler:^(NSArray<AVAssetTrack*>* tracks, NSError* error) {
            result = tracks;
            problem = error;
            dispatch_semaphore_signal(done);
        }];
        if (dispatch_semaphore_wait(done, dispatch_time(DISPATCH_TIME_NOW, 30 * NSEC_PER_SEC))) {
            loaded = false;
            return nil;
        }
        if (problem) { failed(problem); loaded = false; return nil; }
        return result.firstObject;
    }
    // Compatibility with macOS 11, before the asynchronous API was added.
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
    return [asset tracksWithMediaType:type].firstObject;
#pragma clang diagnostic pop
}
CMTime ticks(int64_t time) { return CMTimeMake(time, 10000000); }
struct Media {
    NSURL* file = nil;
    AVURLAsset* asset = nil;
    AVAssetReader* reader = nil;
    AVAssetReaderTrackOutput* video = nil;
    unsigned width = 0, height = 0, rate = 0, channels = 0;
    double seconds = 0;
    std::vector<int16_t> pcm;
    ~Media() {
        [reader cancelReading];
        if (file) [[NSFileManager defaultManager] removeItemAtURL:file error:nil];
    }
};

bool audio(Media& m, AVAssetTrack* track) {
    NSError* error = nil;
    AVAssetReader* reader = [[AVAssetReader alloc] initWithAsset:m.asset error:&error];
    if (!reader) { failed(error); return false; }
    // A predictable interleaved output format, independent of the codec.
    NSDictionary* format = @{
        AVFormatIDKey: @(kAudioFormatLinearPCM), AVSampleRateKey: @44100,
        AVNumberOfChannelsKey: @2, AVLinearPCMBitDepthKey: @16,
        AVLinearPCMIsFloatKey: @NO, AVLinearPCMIsBigEndianKey: @NO,
        AVLinearPCMIsNonInterleaved: @NO,
    };
    AVAssetReaderTrackOutput* output = [[AVAssetReaderTrackOutput alloc]
        initWithTrack:track outputSettings:format];
    if (![reader canAddOutput:output]) return false;
    [reader addOutput:output];
    if (![reader startReading]) { failed(reader.error); return false; }
    while (true) {
        @autoreleasepool {
            CMSampleBufferRef sample = [output copyNextSampleBuffer];
            if (!sample) break;
            CMBlockBufferRef data = CMSampleBufferGetDataBuffer(sample);
            const size_t bytes = data ? CMBlockBufferGetDataLength(data) : 0;
            const size_t at = m.pcm.size();
            m.pcm.resize(at + bytes / sizeof(int16_t));
            const OSStatus status = data && bytes % 2 == 0
                ? CMBlockBufferCopyDataBytes(data, 0, bytes, m.pcm.data() + at) : -1;
            CFRelease(sample);
            if (status) return false;
        }
    }
    if (reader.status == AVAssetReaderStatusFailed) { failed(reader.error); return false; }
    m.rate = 44100;
    m.channels = 2;
    return true;
}

struct Writer {
    AVAssetWriter* writer = nil;
    AVAssetWriterInput* video = nil;
    AVAssetWriterInput* audio = nil;
    AVAssetWriterInputPixelBufferAdaptor* pixels = nil;
    CMAudioFormatDescriptionRef format = nullptr;
    unsigned width = 0, height = 0, channels = 0, rate = 0;
    CMTime audio_end = kCMTimeZero, video_end = kCMTimeZero;
    std::atomic<bool> audio_finished{false}, video_finished{false};
    std::atomic<double> audio_time{0}, video_time{0};
    std::atomic<bool> stopping{false};
    std::thread audio_worker;
    int32_t audio_error = 0;
    ~Writer() {
        // AVFoundation must not be cancelled while a producer is appending.
        stopping = true;
        if (audio_worker.joinable()) audio_worker.join();
        if (writer.status == AVAssetWriterStatusWriting) [writer cancelWriting];
        if (format) CFRelease(format);
    }
};
bool ready(Writer& w, AVAssetWriterInput* input) {
    if (w.stopping) return false;
    const auto deadline = [NSDate dateWithTimeIntervalSinceNow:30.];
    while (!input.readyForMoreMediaData) {
        if (w.stopping) return false;
        if (w.writer.status == AVAssetWriterStatusCancelled) return false;
        if (w.writer.status != AVAssetWriterStatusWriting || deadline.timeIntervalSinceNow <= 0) {
            NSLog(@"ReTzar encoder stalled on %@: video %.3f, audio %.3f, audio finished %d, video finished %d",
                input.mediaType, w.video_time.load(), w.audio_time.load(),
                int(w.audio_finished.load()), int(w.video_finished.load()));
            failed(w.writer.error);
            return false;
        }
        [NSThread sleepForTimeInterval:0.001];
    }
    return !w.stopping;
}
}

extern "C" Media* tz_media_open(const uint8_t* bytes, size_t length,
    const uint16_t* mime, int32_t* error) {
    @autoreleasepool {
        *error = -1;
        auto m = std::make_unique<Media>();
        NSString* kind = text(mime);
        NSString* extension = [kind isEqualToString:@"video/mp4"] ? @"mp4" :
            ([kind isEqualToString:@"video/mpeg"] ? @"mpg" : @"flac");
        m->file = [NSURL fileURLWithPath:[NSTemporaryDirectory() stringByAppendingPathComponent:
            [NSString stringWithFormat:@"retzar-%@.%@", NSUUID.UUID.UUIDString, extension]]];
        NSError* problem = nil;
        if (![[NSData dataWithBytes:bytes length:length] writeToURL:m->file options:0 error:&problem]) {
            failed(problem); return nullptr;
        }
        m->asset = [AVURLAsset URLAssetWithURL:m->file options:nil];
        const auto duration = CMTimeGetSeconds(m->asset.duration);
        m->seconds = std::isfinite(duration) ? std::max(0., duration) : 0.;
        bool loaded = true;
        AVAssetTrack* sound = track(m->asset, AVMediaTypeAudio, loaded);
        if (!loaded) return nullptr;
        if (sound && !audio(*m, sound)) return nullptr;
        AVAssetTrack* picture = track(m->asset, AVMediaTypeVideo, loaded);
        if (!loaded) return nullptr;
        if (picture) {
            const auto size = picture.naturalSize;
            if (size.width <= 0 || size.height <= 0 || size.width > 16384 || size.height > 16384)
                return nullptr;
            m->width = unsigned(size.width);
            m->height = unsigned(size.height);
            m->reader = [[AVAssetReader alloc] initWithAsset:m->asset error:&problem];
            m->video = [[AVAssetReaderTrackOutput alloc] initWithTrack:picture outputSettings:@{
                (id)kCVPixelBufferPixelFormatTypeKey: @(kCVPixelFormatType_32BGRA),
            }];
            if (!m->reader || ![m->reader canAddOutput:m->video]) { failed(problem); return nullptr; }
            [m->reader addOutput:m->video];
            if (![m->reader startReading]) { failed(m->reader.error); return nullptr; }
        } else if (!sound) {
            return nullptr;
        }
        *error = 0;
        return m.release();
    }
}
extern "C" void tz_media_close(Media* m) {
    @autoreleasepool { delete m; }
}
extern "C" void tz_media_info(Media* m, unsigned* width, unsigned* height,
    double* seconds, unsigned* rate, unsigned* channels, size_t* samples) {
    *width = m->width; *height = m->height; *seconds = m->seconds;
    *rate = m->rate; *channels = m->channels; *samples = m->pcm.size();
}
extern "C" void tz_media_pcm(Media* m, int16_t* out) {
    std::copy(m->pcm.begin(), m->pcm.end(), out);
}
extern "C" int32_t tz_media_frame(Media* m, uint8_t* rgba, double* time) {
    @autoreleasepool {
        if (!m->video) return 0;
        CMSampleBufferRef sample = [m->video copyNextSampleBuffer];
        if (!sample) return m->reader.status == AVAssetReaderStatusFailed ? failed(m->reader.error) : 0;
        CVPixelBufferRef image = CMSampleBufferGetImageBuffer(sample);
        if (!image || CVPixelBufferGetWidth(image) != m->width || CVPixelBufferGetHeight(image) != m->height) {
            CFRelease(sample); return -1;
        }
        if (CVPixelBufferLockBaseAddress(image, kCVPixelBufferLock_ReadOnly)) {
            CFRelease(sample); return -1;
        }
        const auto* data = static_cast<const uint8_t*>(CVPixelBufferGetBaseAddress(image));
        const size_t stride = CVPixelBufferGetBytesPerRow(image);
        for (unsigned y = 0; y < m->height; ++y) {
            for (unsigned x = 0; x < m->width; ++x) {
                const auto* in = data + y * stride + x * 4;
                auto* out = rgba + (size_t(y) * m->width + x) * 4;
                out[0] = in[2]; out[1] = in[1]; out[2] = in[0]; out[3] = 255;
            }
        }
        *time = CMTimeGetSeconds(CMSampleBufferGetPresentationTimeStamp(sample));
        CVPixelBufferUnlockBaseAddress(image, kCVPixelBufferLock_ReadOnly);
        CFRelease(sample);
        return 1;
    }
}

extern "C" Writer* tz_mp4_create(const uint16_t* path, unsigned width, unsigned height,
    unsigned fps_num, unsigned fps_den, unsigned bitrate, unsigned quality,
    unsigned rate, unsigned channels, int32_t* error) {
    @autoreleasepool {
        *error = -1;
        if (!width || !height || !fps_num || !fps_den || channels > 2 || (channels && !rate)) return nullptr;
        auto w = std::make_unique<Writer>();
        w->width = width; w->height = height; w->rate = rate; w->channels = channels;
        NSURL* url = [NSURL fileURLWithPath:text(path)];
        NSError* problem = nil;
        // The writer ABI replaces its output, matching Media Foundation.
        NSFileManager* files = [NSFileManager defaultManager];
        BOOL directory = NO;
        if ([files fileExistsAtPath:url.path isDirectory:&directory]) {
            // Do not let an invalid output path remove a directory's contents.
            if (directory) return nullptr;
            if (![files removeItemAtURL:url error:&problem]) { failed(problem); return nullptr; }
        }
        w->writer = [[AVAssetWriter alloc] initWithURL:url fileType:AVFileTypeMPEG4 error:&problem];
        if (!w->writer) { failed(problem); return nullptr; }
        const double fps = double(fps_num) / fps_den;
        // AVFoundation exposes H.264 bitrate rather than the Windows encoder's
        // quality mode. For quality builds use a pixel/fps-scaled bitrate.
        const double target = quality ? std::max(double(bitrate),
            double(width) * height * fps * std::clamp(quality, 1u, 100u) / 100.) : bitrate;
        NSDictionary* video = @{
            AVVideoCodecKey: AVVideoCodecTypeH264, AVVideoWidthKey: @(width), AVVideoHeightKey: @(height),
            AVVideoCompressionPropertiesKey: @{
                AVVideoAverageBitRateKey: @(std::max(64000., target)),
                AVVideoExpectedSourceFrameRateKey: @(fps), AVVideoAllowFrameReorderingKey: @NO,
            },
        };
        w->video = [AVAssetWriterInput assetWriterInputWithMediaType:AVMediaTypeVideo outputSettings:video];
        w->pixels = [AVAssetWriterInputPixelBufferAdaptor assetWriterInputPixelBufferAdaptorWithAssetWriterInput:w->video
            sourcePixelBufferAttributes:@{
                (id)kCVPixelBufferPixelFormatTypeKey: @(kCVPixelFormatType_32BGRA),
                (id)kCVPixelBufferWidthKey: @(width), (id)kCVPixelBufferHeightKey: @(height),
            }];
        if (![w->writer canAddInput:w->video]) return nullptr;
        [w->writer addInput:w->video];
        if (channels) {
            w->audio = [AVAssetWriterInput assetWriterInputWithMediaType:AVMediaTypeAudio outputSettings:@{
                AVFormatIDKey: @(kAudioFormatMPEG4AAC), AVSampleRateKey: @(rate),
                AVNumberOfChannelsKey: @(channels), AVEncoderBitRateKey: @(channels * 96000),
            }];
            if (![w->writer canAddInput:w->audio]) return nullptr;
            [w->writer addInput:w->audio];
            AudioStreamBasicDescription format{};
            format.mSampleRate = rate; format.mFormatID = kAudioFormatLinearPCM;
            format.mFormatFlags = kAudioFormatFlagIsSignedInteger | kAudioFormatFlagIsPacked;
            format.mBytesPerPacket = format.mBytesPerFrame = channels * 2;
            format.mFramesPerPacket = 1; format.mChannelsPerFrame = channels; format.mBitsPerChannel = 16;
            if (CMAudioFormatDescriptionCreate(nullptr, &format, 0, nullptr, 0, nullptr, nullptr, &w->format))
                return nullptr;
        }
        if (![w->writer startWriting]) { failed(w->writer.error); return nullptr; }
        [w->writer startSessionAtSourceTime:kCMTimeZero];
        *error = 0;
        return w.release();
    }
}
extern "C" int32_t tz_mp4_video(Writer* w, const uint8_t* rgba, int64_t time, int64_t duration) {
    @autoreleasepool {
        if (!ready(*w, w->video)) return -1;
        CVPixelBufferRef image = nullptr;
        if (CVPixelBufferPoolCreatePixelBuffer(nullptr, w->pixels.pixelBufferPool, &image)) return -1;
        if (CVPixelBufferLockBaseAddress(image, 0)) { CFRelease(image); return -1; }
        auto* data = static_cast<uint8_t*>(CVPixelBufferGetBaseAddress(image));
        const size_t stride = CVPixelBufferGetBytesPerRow(image);
        for (unsigned y = 0; y < w->height; ++y) {
            for (unsigned x = 0; x < w->width; ++x) {
                const auto* in = rgba + (size_t(y) * w->width + x) * 4;
                auto* out = data + y * stride + x * 4;
                out[0] = in[2]; out[1] = in[1]; out[2] = in[0]; out[3] = 255;
            }
        }
        CVPixelBufferUnlockBaseAddress(image, 0);
        const bool ok = [w->pixels appendPixelBuffer:image withPresentationTime:ticks(time)];
        CFRelease(image);
        w->video_end = CMTimeMaximum(w->video_end, ticks(time + duration));
        w->video_time = double(time + duration) / 1e7;
        return ok ? 0 : failed(w->writer.error);
    }
}
extern "C" int32_t tz_mp4_audio(Writer* w, const int16_t* pcm, unsigned frames,
    int64_t time, int64_t duration) {
    @autoreleasepool {
        if (!w->audio || !frames || !ready(*w, w->audio)) return -1;
        CMBlockBufferRef data = nullptr;
        CMSampleBufferRef sample = nullptr;
        const size_t bytes = size_t(frames) * w->channels * 2;
        OSStatus status = CMBlockBufferCreateWithMemoryBlock(nullptr, nullptr, bytes,
            nullptr, nullptr, 0, bytes, 0, &data);
        if (!status) status = CMBlockBufferReplaceDataBytes(pcm, data, 0, bytes);
        CMSampleTimingInfo timing{CMTimeMake(1, w->rate), ticks(time), kCMTimeInvalid};
        const size_t stride = w->channels * 2;
        if (!status) status = CMSampleBufferCreateReady(nullptr, data, w->format,
            frames, 1, &timing, 1, &stride, &sample);
        const bool ok = !status && [w->audio appendSampleBuffer:sample];
        if (sample) CFRelease(sample);
        if (data) CFRelease(data);
        w->audio_end = CMTimeMaximum(w->audio_end, ticks(time + duration));
        w->audio_time = double(time + duration) / 1e7;
        return ok ? 0 : failed(w->writer.error);
    }
}
extern "C" void tz_mp4_end_audio(Writer* w) {
    @autoreleasepool {
        if (w->audio && !w->audio_finished) {
            [w->audio markAsFinished];
            w->audio_finished = true;
        }
    }
}
extern "C" void tz_mp4_end_video(Writer* w) {
    @autoreleasepool {
        if (!w->video_finished) {
            [w->video markAsFinished];
            w->video_finished = true;
        }
    }
}
// Give the audio producer its entire track before feeding video. Independent
// producers avoid circular backpressure between offline AAC and H.264 encoders.
extern "C" int32_t tz_mp4_queue_audio(Writer* w, const int16_t* pcm, size_t samples) {
    if (!w->audio || !samples || samples % w->channels || w->audio_worker.joinable()) return -1;
    try {
        std::vector<int16_t> copy(pcm, pcm + samples);
        w->audio_worker = std::thread([w, data = std::move(copy)] {
            const size_t stride = w->channels;
            const size_t frames = data.size() / stride;
            for (size_t at = 0; at < frames; at += 1024) {
                const unsigned count = unsigned(std::min(size_t(1024), frames - at));
                const int64_t time = int64_t(std::llround(double(at) / w->rate * 1e7));
                const int64_t duration = int64_t(std::llround(double(count) / w->rate * 1e7));
                w->audio_error = tz_mp4_audio(w, data.data() + at * stride, count, time, duration);
                if (w->audio_error) {
                    w->stopping = true;
                    return;
                }
            }
            tz_mp4_end_audio(w);
        });
        return 0;
    } catch (const std::exception& error) {
        @autoreleasepool { NSLog(@"ReTzar audio encoder: %s", error.what()); }
        return -1;
    }
}
extern "C" int32_t tz_mp4_finish(Writer* writer) {
    @autoreleasepool {
        std::unique_ptr<Writer> w(writer);
        tz_mp4_end_video(w.get());
        if (w->audio_worker.joinable()) w->audio_worker.join();
        if (w->audio_error) return w->audio_error;
        tz_mp4_end_audio(w.get());
        [w->writer endSessionAtSourceTime:CMTimeMaximum(w->audio_end, w->video_end)];
        dispatch_semaphore_t done = dispatch_semaphore_create(0);
        [w->writer finishWritingWithCompletionHandler:^{ dispatch_semaphore_signal(done); }];
        if (dispatch_semaphore_wait(done, dispatch_time(DISPATCH_TIME_NOW, 60 * NSEC_PER_SEC))) {
            [w->writer cancelWriting];
            return -1;
        }
        return w->writer.status == AVAssetWriterStatusCompleted ? 0 : failed(w->writer.error);
    }
}
extern "C" void tz_mp4_abandon(Writer* w) {
    @autoreleasepool { delete w; }
}
