// Real-time exported CLAP host. Reuses native_midi_audio.cpp's SDK/state path.
// HOST PLUGIN STATE BLOCK SECONDS READY_FLAG EVENT_TSV
#include <clap/clap.h>
#include <dlfcn.h>
#include <algorithm>
#include <array>
#include <atomic>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <thread>
#include <vector>

using Clock = std::chrono::steady_clock;
static thread_local bool on_audio_thread = false;
static void require(bool ok, const char* why) {
    if (!ok) { std::fprintf(stderr, "FAIL: %s\n", why); std::exit(1); }
}
struct Stream {
    std::vector<char> bytes; size_t cursor = 0;
    clap_istream_t api{this, [](const clap_istream_t* stream, void* out, uint64_t size)->int64_t {
        auto& s = *static_cast<Stream*>(stream->ctx);
        auto n = std::min<uint64_t>(size, s.bytes.size() - s.cursor);
        std::memcpy(out, s.bytes.data() + s.cursor, n); s.cursor += n; return n;
    }};
};
struct Midi { uint64_t frame; unsigned status, a, b; };
static std::vector<Midi> read_events(const char* path) {
    std::ifstream file(path); require(bool(file), "event plan file");
    std::vector<Midi> result; Midi event{};
    while (file >> event.frame >> event.status >> event.a >> event.b) {
        require(event.status >= 0x80 && event.status <= 0xef && event.a < 128 && event.b < 128, "MIDI plan range");
        require(result.empty() || result.back().frame <= event.frame, "ordered event plan");
        result.push_back(event);
    }
    require(file.eof() && !result.empty(), "complete nonempty event plan"); return result;
}
struct Events {
    std::array<clap_event_midi_t, 128> values{}; uint32_t count = 0;
    clap_input_events_t in{this,
        [](const clap_input_events_t* e)->uint32_t { return static_cast<Events*>(e->ctx)->count; },
        [](const clap_input_events_t* e, uint32_t i)->const clap_event_header_t* {
            auto& s = *static_cast<Events*>(e->ctx); return i < s.count ? &s.values[i].header : nullptr;
        }};
    clap_output_events_t out{this, [](const clap_output_events_t*, const clap_event_header_t*) { return true; }};
    void add(Midi e, uint32_t time) {
        require(count < values.size(), "bounded event list");
        auto& v = values[count++]; v = {};
        v.header = {sizeof(v), time, CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_MIDI, CLAP_EVENT_IS_LIVE};
        v.data[0] = e.status; v.data[1] = e.a; v.data[2] = e.b;
    }
};
struct Host {
    std::thread::id main = std::this_thread::get_id();
    std::atomic<bool> callback{false}, restart{false};
    clap_host_thread_check_t threads{
        [](const clap_host_t* h) { return std::this_thread::get_id() == static_cast<Host*>(h->host_data)->main; },
        [](const clap_host_t*) { return on_audio_thread; }};
    clap_host_t api{CLAP_VERSION, this, "KONTRA live performance gate", "KONTRA", "", "1",
        [](const clap_host_t* h, const char* id)->const void* {
            auto& s = *static_cast<Host*>(h->host_data);
            return std::strcmp(id, CLAP_EXT_THREAD_CHECK) == 0 ? &s.threads : nullptr;
        },
        [](const clap_host_t* h) { static_cast<Host*>(h->host_data)->restart = true; },
        [](const clap_host_t*) {},
        [](const clap_host_t* h) { static_cast<Host*>(h->host_data)->callback = true; }};
};
struct Perf {
    uint64_t busy_ns, span_ns, voices, audible, dropouts, memory, freed, disk_read, underruns, loaded_parts, blocks;
};
using ReadPerf = bool (*)(const clap_plugin_t*, Perf*);

static double quantile(const std::vector<double>& sorted, double q) {
    require(!sorted.empty(), "measured process calls");
    return sorted[std::min(sorted.size()-1, size_t(std::ceil(q * sorted.size())-1))];
}
int main(int argc, char** argv) {
    if (argc == 2 && std::strcmp(argv[1], "--self-check") == 0) {
        require(quantile({1, 2, 3, 4}, .5) == 2 && quantile({1, 2, 3, 4}, .99) == 4, "nearest-rank percentiles");
        Events events; events.add({0, 0x90, 60, 100}, 17);
        require(events.in.size(&events.in) == 1 && events.in.get(&events.in, 0)->time == 17
            && events.in.get(&events.in, 1) == nullptr, "sample-exact bounded host events");
        std::puts("PASS: percentiles and timestamped event input"); return 0;
    }
    require(argc == 8, "PLUGIN STATE BLOCK SECONDS READY_FLAG EVENT_TSV EXPECTED_PARTS");
    const unsigned block = std::strtoul(argv[3], nullptr, 10);
    const double seconds = std::strtod(argv[4], nullptr);
    const unsigned parts = std::strtoul(argv[7], nullptr, 10);
    require(block > 0 && block <= 256 && std::isfinite(seconds) && seconds >= 1 && seconds <= 30 && parts > 0, "bounded host run");
    const auto plan = read_events(argv[6]);
    void* module = dlopen(argv[1], RTLD_NOW | RTLD_LOCAL); require(module, "dlopen plugin");
    auto* entry = static_cast<const clap_plugin_entry_t*>(dlsym(module, "clap_entry"));
    require(entry && entry->init(argv[1]), "CLAP entry");
    auto* factory = static_cast<const clap_plugin_factory_t*>(entry->get_factory(CLAP_PLUGIN_FACTORY_ID));
    require(factory, "CLAP factory"); auto* desc = factory->get_plugin_descriptor(factory, 0); require(desc, "descriptor");
    Host host; auto* p = factory->create_plugin(factory, &host.api, desc->id); require(p && p->init(p), "create/init");
    auto* state = static_cast<const clap_plugin_state_t*>(p->get_extension(p, CLAP_EXT_STATE));
    std::ifstream file(argv[2], std::ios::binary);
    Stream stream{{std::istreambuf_iterator<char>(file), std::istreambuf_iterator<char>()}};
    require(state && !stream.bytes.empty() && state->load(p, &stream.api), "native CLAP state load");
    // Frozen v1 defaults to -12 dB; v2 defaults to 0. Match the actual host parameter.
    auto* params = static_cast<const clap_plugin_params_t*>(p->get_extension(p, CLAP_EXT_PARAMS));
    require(params, "host parameters"); bool volume_set = false;
    for (uint32_t i = 0; i < params->count(p); ++i) {
        clap_param_info_t info{}; require(params->get_info(p, i, &info), "parameter info");
        if (std::strcmp(info.name, "Volume") != 0) continue;
        clap_event_param_value_t value{};
        value.header = {sizeof(value), 0, CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_PARAM_VALUE, 0};
        value.param_id = info.id; value.note_id = value.port_index = value.channel = value.key = -1; value.value = 0;
        clap_input_events_t input{&value, [](const clap_input_events_t*)->uint32_t { return 1; },
            [](const clap_input_events_t* e, uint32_t i)->const clap_event_header_t* { return i == 0 ? &static_cast<clap_event_param_value_t*>(e->ctx)->header : nullptr; }};
        clap_output_events_t output{nullptr, [](const clap_output_events_t*, const clap_event_header_t*) { return true; }};
        params->flush(p, &input, &output); volume_set = true; break;
    }
    require(volume_set, "matched zero-dB master");
    auto* ports = static_cast<const clap_plugin_audio_ports_t*>(p->get_extension(p, CLAP_EXT_AUDIO_PORTS));
    require(ports && ports->count(p, false) > 0 && ports->count(p, false) <= 16, "output port count");
    const auto n = ports->count(p, false);
    std::vector<std::vector<std::array<float, 256>>> pcm(n);
    std::vector<std::vector<float*>> pointers(n); std::vector<clap_audio_buffer_t> outputs(n);
    for (unsigned i = 0; i < n; ++i) {
        clap_audio_port_info_t info{}; require(ports->get(p, i, false, &info) && info.channel_count > 0 && info.channel_count <= 16, "port info");
        pcm[i].resize(info.channel_count); for (auto& c : pcm[i]) pointers[i].push_back(c.data());
        outputs[i] = {pointers[i].data(), nullptr, info.channel_count, 0, 0};
    }
    Events events; clap_process_t process{};
    process.frames_count = block; process.audio_outputs = outputs.data(); process.audio_outputs_count = n;
    process.in_events = &events.in; process.out_events = &events.out;
    clap_event_transport_t transport{};
    transport.header = {sizeof(transport), 0, CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_TRANSPORT, 0};
    transport.flags = CLAP_TRANSPORT_HAS_TEMPO | CLAP_TRANSPORT_HAS_BEATS_TIMELINE | CLAP_TRANSPORT_IS_PLAYING;
    transport.tempo = 120; process.transport = &transport;
    require(p->activate(p, 48000, block, block), "activate");
    auto perf = reinterpret_cast<ReadPerf>(dlsym(module, "__kontra_clap_perf"));
    std::atomic<bool> ready{false}, finished{false};
    std::vector<double> wall, cpu; const uint64_t frames = uint64_t(seconds * 48000);
    wall.reserve(frames / block + 1); cpu.reserve(wall.capacity());
    uint64_t misses = 0, wake_misses = 0, nonfinite = 0, dispatched = 0; double peak = 0;
    auto audio = std::thread([&] {
        on_audio_thread = true; require(p->start_processing(p), "start processing");
        const auto started = Clock::now(); auto deadline = started;
        bool measuring = false; uint64_t at = 0, warm = 0; size_t next = 0;
        while (!measuring || at < frames) {
            const auto period = std::chrono::duration_cast<Clock::duration>(std::chrono::duration<double>(block / 48000.));
            std::this_thread::sleep_until(deadline);
            if (!measuring && ready.load(std::memory_order_acquire)) {
                warm += block; if (warm >= 4800) { measuring = true; deadline = Clock::now(); }
            }
            events.count = 0;
            if (measuring) while (next < plan.size() && plan[next].frame < at + block) {
                require(plan[next].frame >= at, "no late scheduled event");
                events.add(plan[next], uint32_t(plan[next].frame - at)); ++next; ++dispatched;
            }
            timespec c0{}, c1{}; clock_gettime(CLOCK_THREAD_CPUTIME_ID, &c0);
            const auto before = Clock::now();
            require(p->process(p, &process) != CLAP_PROCESS_ERROR, "real CLAP process");
            const auto after = Clock::now(); clock_gettime(CLOCK_THREAD_CPUTIME_ID, &c1);
            if (measuring) {
                const auto us = std::chrono::duration<double, std::micro>(after - before).count(); wall.push_back(us);
                cpu.push_back((c1.tv_sec - c0.tv_sec) * 1e6 + (c1.tv_nsec - c0.tv_nsec) / 1e3);
                misses += us > block * 1e6 / 48000.; wake_misses += before > deadline + period;
                for (const auto& b : pcm) for (const auto& c : b) for (unsigned f = 0; f < block; ++f) {
                    const auto x = c[f]; nonfinite += !std::isfinite(x); if (std::isfinite(x)) peak = std::max(peak, double(std::abs(x)));
                }
                at += block;
            }
            process.steady_time += block;
            transport.song_pos_beats = int64_t(process.steady_time * (120. / 60. / 48000.) * CLAP_BEATTIME_FACTOR);
            deadline += period; // Absolute deadlines: a slow block does not stretch the audition timeline.
            require(Clock::now() - started < std::chrono::seconds(150), "bounded load/readiness wait");
        }
        p->stop_processing(p); finished.store(true, std::memory_order_release);
    });
    Perf previous{}; double ui_cpu = 0, ui_disk = 0; auto sample_at = Clock::now();
    while (!finished.load(std::memory_order_acquire)) {
        if (host.callback.exchange(false)) p->on_main_thread(p);
        require(!host.restart.load(), "unexpected host restart");
        if (perf && Clock::now() - sample_at >= std::chrono::milliseconds(100)) {
            Perf current{}; require(perf(p, &current), "plugin performance readback");
            const auto span = current.span_ns - previous.span_ns;
            ui_cpu = ui_cpu * .5 + .5 * (span ? double(current.busy_ns - previous.busy_ns) / span : 0);
            if (ui_cpu < .005) ui_cpu = 0;
            ui_disk = ui_disk * .5 + .5 * (current.disk_read - previous.disk_read) / 1048576. / std::chrono::duration<double>(Clock::now() - sample_at).count();
            if (ui_disk < .05) ui_disk = 0;
            std::printf("{\"kind\":\"perf_view\",\"cpu_percent\":%.6f,\"disk_mb_s\":%.6f,\"voices\":%llu,\"audible\":%llu,\"dropouts\":%llu,\"sample_ram_bytes\":%llu,\"freed_bytes\":%llu,\"underruns\":%llu,\"loaded_parts\":%llu,\"blocks\":%llu}\n",
                ui_cpu * 100, ui_disk, (unsigned long long)current.voices, (unsigned long long)current.audible, (unsigned long long)current.dropouts,
                (unsigned long long)current.memory, (unsigned long long)current.freed, (unsigned long long)current.underruns, (unsigned long long)current.loaded_parts, (unsigned long long)current.blocks);
            if (current.loaded_parts >= parts) ready.store(true, std::memory_order_release);
            previous = current; sample_at = Clock::now();
        }
        if (!perf && std::ifstream(argv[5]).good()) ready.store(true, std::memory_order_release);
        std::this_thread::sleep_for(std::chrono::milliseconds(1));
    }
    audio.join(); if (host.callback.exchange(false)) p->on_main_thread(p);
    // Let the one-second frozen-v1 diagnostic snapshot reach its existing worker.
    std::this_thread::sleep_for(std::chrono::milliseconds(200));
    std::sort(wall.begin(), wall.end()); std::sort(cpu.begin(), cpu.end());
    std::printf("{\"kind\":\"live_host\",\"block\":%u,\"seconds\":%.3f,\"blocks\":%zu,\"cpu_p50_us\":%.3f,\"cpu_p99_us\":%.3f,\"thread_cpu_p50_us\":%.3f,\"thread_cpu_p99_us\":%.3f,\"deadline_misses\":%llu,\"wake_deadline_misses\":%llu,\"peak\":%.9g,\"nonfinite\":%llu,\"events_dispatched\":%llu,\"events_planned\":%zu,\"perf_view_available\":%s}\n",
        block, seconds, wall.size(), quantile(wall, .5), quantile(wall, .99), quantile(cpu, .5), quantile(cpu, .99),
        (unsigned long long)misses, (unsigned long long)wake_misses, peak, (unsigned long long)nonfinite, (unsigned long long)dispatched, plan.size(), perf ? "true" : "false");
    p->deactivate(p); p->destroy(p); entry->deinit(); dlclose(module);
    require(dispatched == plan.size() && nonfinite == 0, "complete finite audition");
}
