// Per-note fingerprints; PCM is bounded RAM only and never serialized.
#pragma once
#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <vector>

struct FamilyAudio {
    std::vector<std::array<float, 2>> pcm;
    struct Metrics {
        int64_t onset = -1, last = -1;
        double peak = 0, rms = 0;
        std::array<double, 32> spectrum{};
    };
    static Metrics measure(const std::vector<std::array<float, 2>>& samples, size_t begin, size_t end) {
        Metrics m;
        for (size_t i = begin; i < end; ++i) for (float x : samples[i]) m.peak = std::max(m.peak, double(std::abs(x)));
        if (m.peak == 0) return m;
        double energy = 0;
        for (size_t i = begin; i < end; ++i) {
            for (float x : samples[i]) energy += double(x) * x;
            if (std::max(std::abs(samples[i][0]), std::abs(samples[i][1])) >= m.peak * 1e-4) {
                if (m.onset < 0) m.onset = i - begin;
                m.last = i - begin;
            }
        }
        m.rms = std::sqrt(energy / (2 * (end - begin)));
        // Fixed log-frequency Goertzel powers preserve stereo energy without phase cancellation.
        double total = 0;
        for (size_t band = 0; band < m.spectrum.size(); ++band) {
            double coefficient = 2 * std::cos(2 * 3.14159265358979323846 * (62.5 * std::pow(256., band / 31.)) / 48000.);
            for (size_t channel = 0; channel < 2; ++channel) {
                double a = 0, b = 0;
                for (size_t i = begin; i < end; ++i) {
                    double c = samples[i][channel] + coefficient * a - b; b = a; a = c;
                }
                m.spectrum[band] += std::max(0., a * a + b * b - coefficient * a * b);
            }
            total += m.spectrum[band];
        }
        if (total > 0) for (auto& band : m.spectrum) band /= total;
        return m;
    }
    template<class Events> void report(const Events& events) const {
        auto full = measure(pcm, 0, pcm.size());
        std::printf("{\"kind\":\"family_audio\",\"window_frames\":%zu,\"onset_frame\":%lld,\"length_frames\":%lld,\"peak\":%.9g,\"rms\":%.9g,\"spectrum\":[", pcm.size(), (long long)full.onset, (long long)(full.last < 0 ? 0 : full.last - full.onset + 1), full.peak, full.rms);
        for (size_t band = 0; band < full.spectrum.size(); ++band) std::printf("%s%.9g", band ? "," : "", full.spectrum[band]);
        std::puts("]}");
        size_t cached_begin = pcm.size(), cached_end = pcm.size();
        Metrics cached;
        for (size_t note = 0; note < events.size(); ++note) {
            const auto& event = events[note];
            if ((event.status & 0xf0) != 0x90 || event.b == 0 || event.frame >= pcm.size()) continue;
            size_t end = pcm.size();
            for (size_t next = note + 1; next < events.size(); ++next) if ((events[next].status & 0xf0) == 0x90 && events[next].b > 0 && events[next].frame > event.frame) { end = std::min(end, size_t(events[next].frame)); break; }
            if (end <= event.frame) continue;
            if (cached_begin != event.frame || cached_end != end) { cached = measure(pcm, event.frame, end); cached_begin = event.frame; cached_end = end; }
            const auto& m = cached;
            std::printf("{\"kind\":\"note_audio\",\"event_index\":%zu,\"key\":%u,\"velocity\":%u,\"start_frame\":%llu,\"window_frames\":%zu,\"onset_frame\":%lld,\"length_frames\":%lld,\"peak\":%.9g,\"rms\":%.9g,\"spectrum\":[", note, event.a, event.b, (unsigned long long)event.frame, end - event.frame, (long long)m.onset, (long long)(m.last < 0 ? 0 : m.last - m.onset + 1), m.peak, m.rms);
            for (size_t band = 0; band < m.spectrum.size(); ++band) std::printf("%s%.9g", band ? "," : "", m.spectrum[band]);
            std::puts("]}");
        }
    }
};
