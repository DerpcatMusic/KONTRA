// W11 owned adapter copy of e0e1d35f presented-host.cpp primitives.
// Baseline presented-host.cpp remains unchanged.
// HOST PLUGIN NATIVE_STATE --gestures; bounded commands on stdin.
// HOST PLUGIN --template TMPFS_FILE for this exact plugin envelope.
// Stdout/stderr, native state and optional RGB must remain in RAM/pipe.
// Compile with official CLAP headers, -lX11 -lXtst -ldl -pthread.
#include "widget-host-plan.hpp"
#include <X11/Xatom.h>
#include <X11/Xlib.h>
#include <X11/Xutil.h>
#include <X11/extensions/XTest.h>
#include <algorithm>
#include <array>
#include <atomic>
#include <cerrno>
#include <chrono>
#include <clap/clap.h>
#include <cmath>
#include <csignal>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <dlfcn.h>
#include <fcntl.h>
#include <fstream>
#include <iostream>
#include <linux/magic.h>
#include <map>
#include <mutex>
#include <poll.h>
#include <set>
#include <sys/stat.h>
#include <sys/vfs.h>
#include <thread>
#include <unistd.h>
#include <vector>

using Clock = std::chrono::steady_clock;
static thread_local bool audio_thread = false;
static volatile sig_atomic_t requested_shutdown = 0;
static volatile sig_atomic_t x_error = 0;
static void require(bool ok, const char *stage) {
    if (!ok)
        throw std::runtime_error(stage);
}
struct Stream {
    std::vector<char> bytes;
    size_t cursor = 0;
    clap_istream_t api{this, [](const clap_istream_t *in, void *out, uint64_t size) -> int64_t {
                           auto &s = *static_cast<Stream *>(in->ctx);
                           auto n = std::min<uint64_t>(size, s.bytes.size() - s.cursor);
                           std::memcpy(out, s.bytes.data() + s.cursor, n);
                           s.cursor += n;
                           return n;
                       }};
};
struct SavedState {
    std::vector<char> bytes;
    clap_ostream_t api{this,
                       [](const clap_ostream_t *out, const void *data, uint64_t size) -> int64_t {
                           auto &s = *static_cast<SavedState *>(out->ctx);
                           if (size > 64 * 1024 * 1024 - s.bytes.size())
                               return -1;
                           if (!size)
                               return 0;
                           const auto *start = static_cast<const char *>(data);
                           s.bytes.insert(s.bytes.end(), start, start + size);
                           return size;
                       }};
};
static void save_state(const clap_plugin_t *p, const clap_plugin_state_t *state,
                       const std::string &path) {
    SavedState saved;
    require(state && state->save(p, &saved.api), "native state save");
    const int fd = ::open(path.c_str(), O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC, 0600);
    require(fd >= 0, "new RAM template file");
    struct statfs volume{};
    if (fstatfs(fd, &volume) != 0 || volume.f_type != TMPFS_MAGIC) {
        close(fd);
        require(false, "RAM template filesystem");
    }
    size_t cursor = 0;
    while (cursor < saved.bytes.size()) {
        const auto size = write(fd, saved.bytes.data() + cursor, saved.bytes.size() - cursor);
        if (size < 0 && errno == EINTR)
            continue;
        if (size <= 0) {
            close(fd);
            require(false, "RAM template write");
        }
        cursor += size;
    }
    require(close(fd) == 0, "RAM template close");
}
struct Host {
    std::thread::id main = std::this_thread::get_id();
    std::atomic<bool> callback{false};
    std::atomic<uint64_t> resize{0};
    bool fixed_rss = false;
    clap_host_gui_t gui{
        [](const clap_host_t *) {},
        [](const clap_host_t *h, uint32_t w, uint32_t height) {
            if (!w || !height || w > 4096 || height > 2160)
                return false;
            auto &host = *static_cast<Host *>(h->host_data);
            const bool accepted = !host.fixed_rss || (w == 1180 && height == 760);
            if (host.fixed_rss) {
                std::printf(
                    "{\"kind\":\"resize_request\",\"width\":%u,\"height\":%u,\"accepted\":%s}\n", w,
                    height, accepted ? "true" : "false");
                std::fflush(stdout);
            }
            if (!accepted)
                return false;
            host.resize = (uint64_t(w) << 32) | height;
            return true;
        },
        [](const clap_host_t *) { return true; }, [](const clap_host_t *) { return false; },
        [](const clap_host_t *, bool) {}};
    clap_host_thread_check_t threads{[](const clap_host_t *h) {
                                         return std::this_thread::get_id() ==
                                                static_cast<Host *>(h->host_data)->main;
                                     },
                                     [](const clap_host_t *) { return audio_thread; }};
    clap_host_t api{
        CLAP_VERSION,
        this,
        "KONTRA presented gate",
        "KONTRA",
        "",
        "1",
        [](const clap_host_t *h, const char *id) -> const void * {
            auto &s = *static_cast<Host *>(h->host_data);
            if (std::strcmp(id, CLAP_EXT_THREAD_CHECK) == 0)
                return &s.threads;
            return std::strcmp(id, CLAP_EXT_GUI) == 0 ? &s.gui : nullptr;
        },
        [](const clap_host_t *) {},
        [](const clap_host_t *) {},
        [](const clap_host_t *h) { static_cast<Host *>(h->host_data)->callback = true; }};
};
struct Perf {
    uint64_t busy, span, voices, audible, dropouts, memory, freed, disk, underruns, loaded, blocks;
};
static Window open_editor(const clap_plugin_t *p, const clap_plugin_gui_t *gui, Display *display,
                          uint32_t &width, uint32_t &height, bool rss, bool &created,
                          Window &allocated) {
    require(gui && gui->is_api_supported(p, CLAP_WINDOW_API_X11, false) &&
                gui->create(p, CLAP_WINDOW_API_X11, false),
            "GUI create");
    created = true;
    require(gui->get_size(p, &width, &height), "GUI size");
    require(width > 0 && height > 0 && width <= 4096 && height <= 2160, "GUI bounds");
    if (rss) {
        require(gui->set_scale(p, 1.0), "GUI scale 1");
        width = 1180;
        height = 760;
        require(gui->adjust_size(p, &width, &height) && gui->set_size(p, width, height),
                "GUI fixed size");
        require(width == 1180 && height == 760, "matched GUI size");
    }
    const auto window =
        XCreateSimpleWindow(display, DefaultRootWindow(display), 20, 20, width, height, 0, 0, 0);
    allocated = window;
    const unsigned long pid = getpid();
    XChangeProperty(display, window, XInternAtom(display, "_NET_WM_PID", False), XA_CARDINAL, 32,
                    PropModeReplace, reinterpret_cast<const unsigned char *>(&pid), 1);
    if (rss) {
        // Tiling WMs may replace the requested size on map; keep this measurement viewport fixed.
        XSetWindowAttributes attributes{};
        attributes.override_redirect = True;
        XChangeWindowAttributes(display, window, CWOverrideRedirect, &attributes);
    }
    XSelectInput(display, window, StructureNotifyMask);
    if (rss) {
        XSync(display, False);
        XWindowAttributes attr{};
        require(XGetWindowAttributes(display, window, &attr), "created parent geometry");
        std::printf("{\"kind\":\"created_parent\",\"width\":%d,\"height\":%d,\"border\":%d,"
                    "\"override_redirect\":%s}\n",
                    attr.width, attr.height, attr.border_width,
                    attr.override_redirect ? "true" : "false");
        std::fflush(stdout);
    }
    XStoreName(display, window, "KONTRA presented gate");
    XMapRaised(display, window);
    XSync(display, False);
    clap_window_t parent{};
    parent.api = CLAP_WINDOW_API_X11;
    parent.x11 = window;
    require(gui->set_parent(p, &parent) && gui->show(p), "GUI attach/show");
    std::fprintf(stderr, "presented window=%lu size=%ux%u\n", window, width, height);
    return window;
}
static void pump(const clap_plugin_t *p, const clap_plugin_gui_t *gui, Host &host, Display *display,
                 Window window) {
    if (host.callback.exchange(false))
        p->on_main_thread(p);
    while (XPending(display)) {
        XEvent event{};
        XNextEvent(display, &event);
        if (window && event.type == ConfigureNotify && event.xconfigure.window == window) {
            uint32_t w = event.xconfigure.width, h = event.xconfigure.height;
            if (host.fixed_rss) {
                std::printf("{\"kind\":\"parent_configure\",\"width\":%u,\"height\":%u,\"border\":%"
                            "d,\"synthetic\":%s}\n",
                            w, h, event.xconfigure.border_width,
                            event.xconfigure.send_event ? "true" : "false");
                std::fflush(stdout);
            }
            require(gui->set_size(p, w, h), "host resize");
        }
    }
    if (const auto size = host.resize.exchange(0); window && size)
        XResizeWindow(display, window, size >> 32, size & 0xffffffff);
}
static void ram_fd(int fd) {
    struct stat info{};
    struct statfs volume{};
    require(fstat(fd, &info) == 0, "RAM descriptor");
    require(S_ISFIFO(info.st_mode) || S_ISSOCK(info.st_mode) ||
                (fstatfs(fd, &volume) == 0 && volume.f_type == TMPFS_MAGIC),
            "RAM-only descriptor");
}
static void ram_path(const char *path, bool existing) {
    const std::string value(path);
    const auto slash = value.find_last_of('/');
    require(slash != std::string::npos, "absolute RAM state path");
    struct statfs volume{};
    require(statfs(existing ? path : value.substr(0, slash).c_str(), &volume) == 0 &&
                volume.f_type == TMPFS_MAGIC,
            "RAM-only state path");
}
struct GestureSession {
    Display *display;
    const clap_plugin_factory_t *factory;
    const clap_plugin_descriptor_t *descriptor;
    Host host;
    const clap_plugin_t *plugin = nullptr;
    const clap_plugin_state_t *state = nullptr;
    const clap_plugin_gui_t *gui = nullptr;
    const clap_plugin_params_t *params = nullptr;
    bool active = false, saved_valid = false, gui_created = false;
    Window window = 0;
    uint32_t width = 0, height = 0, generation = 0;
    SavedState saved;
    std::set<unsigned> keys, buttons;
    std::atomic<bool> running{false}, started{false}, failed{false};
    std::thread audio;
    std::mutex lock;
    std::vector<clap_event_midi_t> pending;
    uint64_t blocks = 0, samples = 0, param_events = 0, nonfinite = 0;
    double energy = 0, peak = 0;
    std::vector<std::vector<std::array<float, 64>>> pcm;
    std::vector<std::vector<float *>> pointers;
    std::vector<clap_audio_buffer_t> outputs;
    bool (*perf)(const clap_plugin_t *, Perf *) = nullptr;
    int rgb_fd = -1;

    GestureSession(Display *d, const clap_plugin_factory_t *f, const clap_plugin_descriptor_t *desc)
        : display(d), factory(f), descriptor(desc) {}
    ~GestureSession() {
        // Error/EOF cleanup releases OS input and joins processing before destruction.
        release_inputs();
        stop_audio();
        if (gui_created && plugin && gui) {
            gui->hide(plugin);
            gui->destroy(plugin);
        }
        if (window)
            XDestroyWindow(display, window);
        if (plugin) {
            if (active)
                plugin->deactivate(plugin);
            plugin->destroy(plugin);
        }
        XSync(display, False);
    }
    void release_inputs() {
        for (const auto key : keys)
            XTestFakeKeyEvent(display, key, False, CurrentTime);
        for (const auto button : buttons)
            XTestFakeButtonEvent(display, button, False, CurrentTime);
        keys.clear();
        buttons.clear();
        XSync(display, False);
    }
    void reset_host() {
        host.callback = false;
        host.resize = 0;
    }
    void create_plugin() {
        plugin = factory->create_plugin(factory, &host.api, descriptor->id);
        require(plugin && plugin->init(plugin), "fresh plugin init");
        ++generation;
        state =
            static_cast<const clap_plugin_state_t *>(plugin->get_extension(plugin, CLAP_EXT_STATE));
        gui = static_cast<const clap_plugin_gui_t *>(plugin->get_extension(plugin, CLAP_EXT_GUI));
        params = static_cast<const clap_plugin_params_t *>(
            plugin->get_extension(plugin, CLAP_EXT_PARAMS));
        require(state && gui, "state and GUI extensions");
    }
    void load_saved_state() {
        require(saved_valid && !saved.bytes.empty() && !active, "saved inactive state");
        Stream stream{saved.bytes};
        require(state->load(plugin, &stream.api), "fresh plugin state load");
    }
    void save() {
        release_inputs();
        saved.bytes.clear();
        require(state->save(plugin, &saved.api) && !saved.bytes.empty(), "state save in RAM");
        saved_valid = true;
    }
    void start_audio() {
        auto *ports = static_cast<const clap_plugin_audio_ports_t *>(
            plugin->get_extension(plugin, CLAP_EXT_AUDIO_PORTS));
        require(ports, "audio ports");
        const auto buses = ports->count(plugin, false);
        require(buses > 0 && buses <= 16, "bus bound");
        pcm.clear();
        pointers.clear();
        outputs.clear();
        pcm.resize(buses);
        pointers.resize(buses);
        outputs.resize(buses);
        for (uint32_t b = 0; b < buses; ++b) {
            clap_audio_port_info_t info{};
            require(ports->get(plugin, b, false, &info) && info.channel_count > 0 &&
                        info.channel_count <= 16,
                    "channel bound");
            pcm[b].resize(info.channel_count);
            for (auto &channel : pcm[b])
                pointers[b].push_back(channel.data());
            outputs[b] = {pointers[b].data(), nullptr, info.channel_count, 0, 0};
        }
        require(plugin->activate(plugin, 48000, 64, 64), "activate");
        active = true;
        running = true;
        started = false;
        failed = false;
        {
            std::lock_guard guard(lock);
            pending.clear();
            blocks = samples = param_events = nonfinite = 0;
            energy = peak = 0;
        }
        audio = std::thread([this] {
            audio_thread = true;
            if (!plugin->start_processing(plugin)) {
                failed = true;
                return;
            }
            started = true;
            std::vector<clap_event_midi_t> events;
            clap_input_events_t input{
                &events,
                [](const clap_input_events_t *in) -> uint32_t {
                    return static_cast<std::vector<clap_event_midi_t> *>(in->ctx)->size();
                },
                [](const clap_input_events_t *in, uint32_t at) -> const clap_event_header_t * {
                    auto &e = *static_cast<std::vector<clap_event_midi_t> *>(in->ctx);
                    return at < e.size() ? &e[at].header : nullptr;
                }};
            clap_output_events_t output{
                this, [](const clap_output_events_t *out, const clap_event_header_t *event) {
                    auto &session = *static_cast<GestureSession *>(out->ctx);
                    if (event && event->space_id == CLAP_CORE_EVENT_SPACE_ID &&
                        event->type == CLAP_EVENT_PARAM_VALUE) {
                        std::lock_guard guard(session.lock);
                        ++session.param_events;
                    }
                    return true;
                }};
            clap_process_t process{};
            process.frames_count = 64;
            process.audio_outputs = outputs.data();
            process.audio_outputs_count = outputs.size();
            process.in_events = &input;
            process.out_events = &output;
            auto next = Clock::now();
            while (running) {
                {
                    std::lock_guard guard(lock);
                    events.swap(pending);
                }
                const auto result = plugin->process(plugin, &process);
                if (result == CLAP_PROCESS_ERROR) {
                    failed = true;
                    break;
                }
                double block_energy = 0, block_peak = 0;
                uint64_t count = 0, invalid = 0;
                for (size_t b = 0; b < pcm.size(); ++b)
                    for (size_t c = 0; c < pcm[b].size(); ++c)
                        for (size_t sample = 0; sample < 64; ++sample) {
                            const auto value =
                                pcm[b][c]
                                   [(outputs[b].constant_mask & (uint64_t(1) << c)) ? 0 : sample];
                            if (std::isfinite(value)) {
                                block_energy += double(value) * value;
                                block_peak = std::max(block_peak, std::abs(double(value)));
                            } else
                                ++invalid;
                            ++count;
                        }
                {
                    std::lock_guard guard(lock);
                    ++blocks;
                    samples += count;
                    energy += block_energy;
                    peak = std::max(peak, block_peak);
                    nonfinite += invalid;
                }
                events.clear();
                process.steady_time += 64;
                next += std::chrono::nanoseconds(1333333);
                std::this_thread::sleep_until(next);
            }
            plugin->stop_processing(plugin);
        });
    }
    void stop_audio() {
        running = false;
        if (audio.joinable())
            audio.join();
    }
    void destroy_plugin() {
        require(!audio.joinable() && !window && !gui_created, "plugin destruction barrier");
        if (active) {
            plugin->deactivate(plugin);
            active = false;
        }
        plugin->destroy(plugin);
        plugin = nullptr;
        state = nullptr;
        gui = nullptr;
        params = nullptr;
    }
    void pump_once() {
        require(!requested_shutdown, "interrupted");
        require(!failed, "audio process fault");
        require(!x_error, "X11 error");
        pump(plugin, gui, host, display, window);
        require(!x_error, "X11 error");
    }
    void wait(unsigned ms) {
        const auto end = Clock::now() + std::chrono::milliseconds(ms);
        do {
            pump_once();
            std::this_thread::sleep_for(std::chrono::milliseconds(5));
        } while (Clock::now() < end);
    }
    void await_ready() {
        const auto end = Clock::now() + std::chrono::seconds(120);
        Perf metrics{};
        while (!started || (perf && (!perf(plugin, &metrics) || metrics.loaded != 1))) {
            require(Clock::now() < end, "load deadline");
            wait(10);
        }
        // Without a plugin readiness export, CLAP load returning is not certified
        // as asynchronous library readiness. Driver receipts must keep it UNKNOWN.
    }
    Window child() {
        Window root = 0, parent = 0, *children = nullptr;
        unsigned count = 0;
        require(window && XQueryTree(display, window, &root, &parent, &children, &count),
                "editor child query");
        const auto result = count == 1 ? children[0] : Window(0);
        if (children)
            XFree(children);
        require(result, "one editor child");
        return result;
    }
    Window geometry(int &root_x, int &root_y) {
        const auto surface = child();
        XWindowAttributes attr{};
        Window ignored = 0;
        require(XGetWindowAttributes(display, surface, &attr) && attr.map_state == IsViewable &&
                    attr.width > 0 && attr.height > 0 && attr.width <= 4096 && attr.height <= 2160,
                "mapped editor bounds");
        require(XTranslateCoordinates(display, surface, DefaultRootWindow(display), 0, 0, &root_x,
                                      &root_y, &ignored),
                "child root translation");
        width = attr.width;
        height = attr.height;
        return surface;
    }
    void open_editor() {
        window = ::open_editor(plugin, gui, display, width, height, false, gui_created, window);
        const auto end = Clock::now() + std::chrono::seconds(10);
        while (true) {
            Window root = 0, parent = 0, *children = nullptr;
            unsigned count = 0;
            XQueryTree(display, window, &root, &parent, &children, &count);
            XWindowAttributes attr{};
            const bool mapped = count == 1 && XGetWindowAttributes(display, children[0], &attr) &&
                                attr.map_state == IsViewable;
            if (children)
                XFree(children);
            if (mapped)
                break;
            require(Clock::now() < end, "editor map deadline");
            wait(10);
        }
        int x, y;
        const auto surface = geometry(x, y);
        XSetInputFocus(display, surface, RevertToParent, CurrentTime);
        XSync(display, False);
    }
    void close_editor() {
        require(!audio.joinable(), "editor destruction barrier");
        require(gui->hide(plugin), "GUI hide");
        gui->destroy(plugin);
        gui_created = false;
        XDestroyWindow(display, window);
        XSync(display, False);
        window = 0;
    }
    void screenshot(uint32_t sequence) {
        if (rgb_fd < 0)
            return;
        int x, y;
        const auto surface = geometry(x, y);
        auto *image = XGetImage(display, surface, 0, 0, width, height, AllPlanes, ZPixmap);
        require(image, "RGB readback");
        if (image->red_mask != 0xff0000 || image->green_mask != 0xff00 ||
            image->blue_mask != 0xff) {
            XDestroyImage(image);
            require(false, "RGB masks");
        }
        std::vector<unsigned char> bytes(width * height * 3);
        for (unsigned y = 0; y < height; ++y)
            for (unsigned x = 0; x < width; ++x) {
                const auto pixel = XGetPixel(image, x, y);
                auto *rgb = &bytes[(y * width + x) * 3];
                rgb[0] = (pixel >> 16) & 255;
                rgb[1] = (pixel >> 8) & 255;
                rgb[2] = pixel & 255;
            }
        XDestroyImage(image);
        auto write_all = [this](const void *data, size_t length) {
            const auto *start = static_cast<const unsigned char *>(data);
            while (length) {
                require(!requested_shutdown, "interrupted RGB write");
                const auto written = write(rgb_fd, start, length);
                if (written < 0 && errno == EINTR)
                    continue;
                require(written > 0, "RGB pipe write");
                start += written;
                length -= written;
            }
        };
        const uint32_t header[] = {sequence, generation, width, height};
        write_all(header, sizeof(header));
        write_all(bytes.data(), bytes.size());
    }
    void checkpoint(uint32_t sequence) {
        int x, y;
        geometry(x, y);
        pump_once();
        std::printf("{\"schema\":1,\"kind\":\"checkpoint\",\"sequence\":%u,\"generation\":%u,"
                    "\"width\":%u,\"height\":%u,\"load_ready\":%s,\"parameters\":[",
                    sequence, generation, width, height, perf ? "true" : "false");
        const auto count = params ? params->count(plugin) : 0;
        require(count <= 65536, "parameter bound");
        bool first = true;
        for (uint32_t i = 0; i < count; ++i) {
            clap_param_info_t info{};
            double value;
            require(params->get_info(plugin, i, &info) &&
                        params->get_value(plugin, info.id, &value) && std::isfinite(value),
                    "parameter readback");
            std::printf("%s[%u,%.17g]", first ? "" : ",", info.id, value);
            first = false;
        }
        {
            std::lock_guard guard(lock);
            std::printf("],\"blocks\":%llu,\"samples\":%llu,\"rms\":%.17g,\"peak\":%.17g,"
                        "\"nonfinite\":%llu,\"parameter_events\":%llu}\n",
                        (unsigned long long)blocks, (unsigned long long)samples,
                        samples ? std::sqrt(energy / samples) : 0, peak,
                        (unsigned long long)nonfinite, (unsigned long long)param_events);
            blocks = samples = param_events = nonfinite = 0;
            energy = peak = 0;
        }
        require(std::fflush(stdout) == 0, "checkpoint pipe write");
        screenshot(sequence);
    }
    bool dispatch(const widget_host::Command &command) {
        using widget_host::Op;
        const auto &a = command.args;
        if (command.op == Op::Move) {
            int x, y;
            geometry(x, y);
            const auto at = widget_host::root_point(command, width, height, x, y);
            require(XTestFakeMotionEvent(display, -1, at[0], at[1], CurrentTime),
                    "OS pointer event");
            XSync(display, False);
        } else if (command.op == Op::Button || command.op == Op::Key) {
            int x, y;
            const auto surface = geometry(x, y);
            XSetInputFocus(display, surface, RevertToParent, CurrentTime);
            if (command.op == Op::Button) {
                Window root, child;
                int rx, ry, wx, wy;
                unsigned mask;
                require(XQueryPointer(display, surface, &root, &child, &rx, &ry, &wx, &wy, &mask) &&
                            wx >= 0 && wy >= 0 && wx < int(width) && wy < int(height),
                        "pointer inside editor");
                const auto button = unsigned(a[0]);
                if (a[1])
                    buttons.insert(button);
                else
                    buttons.erase(button);
                require(XTestFakeButtonEvent(display, button, a[1], CurrentTime),
                        "OS button event");
            } else {
                const auto key = XKeysymToKeycode(display, KeySym(a[0]));
                require(key, "OS keysym mapping");
                if (a[1])
                    keys.insert(key);
                else
                    keys.erase(key);
                require(XTestFakeKeyEvent(display, key, a[1], CurrentTime), "OS key event");
            }
            XSync(display, False);
        } else if (command.op == Op::Wait)
            wait(a[0]);
        else if (command.op == Op::Checkpoint)
            checkpoint(command.sequence);
        else if (command.op == Op::Save) {
            save();
            std::printf("{\"schema\":1,\"kind\":\"saved\",\"sequence\":%u,\"generation\":%u,"
                        "\"state_bytes\":%zu}\n",
                        command.sequence, generation, saved.bytes.size());
            std::fflush(stdout);
        } else if (command.op == Op::Reload) {
            require(saved_valid, "save before fresh reload");
            widget_host::fresh_reload(*this);
            std::printf("{\"schema\":1,\"kind\":\"reloaded\",\"sequence\":%u,\"generation\":%u}\n",
                        command.sequence, generation);
            std::fflush(stdout);
        } else if (command.op == Op::Midi) {
            clap_event_midi_t event{};
            event.header = {sizeof(event), 0, CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_MIDI, 0};
            event.port_index = a[0];
            for (unsigned i = 0; i < 3; ++i)
                event.data[i] = a[i + 1];
            std::lock_guard guard(lock);
            require(pending.size() < 512, "MIDI queue bound");
            pending.push_back(event);
        } else
            return false;
        return true;
    }
    void plan() {
        uint32_t sequence = 0;
        std::string buffered;
        const auto end = Clock::now() + std::chrono::seconds(300);
        while (true) {
            require(Clock::now() < end, "plan deadline");
            pump_once();
            pollfd input{STDIN_FILENO, POLLIN, 0};
            const auto result = poll(&input, 1, 5);
            require(result >= 0 || errno == EINTR, "plan poll");
            if (result <= 0)
                continue;
            require(!(input.revents & (POLLERR | POLLNVAL)), "plan pipe");
            char bytes[512];
            const auto length = read(STDIN_FILENO, bytes, sizeof(bytes));
            if (length < 0 && errno == EINTR)
                continue;
            require(length >= 0, "plan read");
            if (!length) {
                require(buffered.empty(), "plan final newline");
                return;
            }
            buffered.append(bytes, length);
            while (true) {
                const auto newline = buffered.find('\n');
                if (newline == std::string::npos)
                    break;
                const auto command = widget_host::parse(buffered.substr(0, newline));
                buffered.erase(0, newline + 1);
                require(command.sequence == sequence++, "ordered plan sequence");
                if (!dispatch(command))
                    return;
            }
            require(buffered.size() <= 256, "plan buffered line bound");
        }
    }
};
int main(int argc, char **argv) {
    Display *display = nullptr;
    void *module = nullptr;
    const clap_plugin_entry_t *entry = nullptr;
    bool initialized = false;
    try {
        require(argc == 4, "PLUGIN TMPFS_NATIVE_STATE --gestures or PLUGIN --template TMPFS_STATE");
        std::signal(SIGPIPE, SIG_IGN);
        struct sigaction action{};
        action.sa_handler = [](int) { requested_shutdown = 1; };
        sigemptyset(&action.sa_mask);
        require(sigaction(SIGTERM, &action, nullptr) == 0 &&
                    sigaction(SIGINT, &action, nullptr) == 0,
                "signal cleanup handler");
        XSetErrorHandler([](Display *, XErrorEvent *event) {
            x_error = event->error_code;
            return 0;
        });
        ram_fd(STDIN_FILENO);
        ram_fd(STDOUT_FILENO);
        ram_fd(STDERR_FILENO);
        require(XInitThreads(), "X11 threads");
        display = XOpenDisplay(nullptr);
        require(display, "X11 display");
        module = dlopen(argv[1], RTLD_NOW | RTLD_LOCAL);
        require(module, "module");
        entry = static_cast<const clap_plugin_entry_t *>(dlsym(module, "clap_entry"));
        require(entry && entry->init(argv[1]), "entry");
        initialized = true;
        auto *factory =
            static_cast<const clap_plugin_factory_t *>(entry->get_factory(CLAP_PLUGIN_FACTORY_ID));
        require(factory, "factory");
        auto *descriptor = factory->get_plugin_descriptor(factory, 0);
        require(descriptor, "descriptor");
        {
            GestureSession session(display, factory, descriptor);
            session.create_plugin();
            if (std::strcmp(argv[2], "--template") == 0) {
                ram_path(argv[3], false);
                save_state(session.plugin, session.state, argv[3]);
                std::printf("{\"schema\":1,\"kind\":\"template\"}\n");
                std::fflush(stdout);
            } else {
                require(std::strcmp(argv[3], "--gestures") == 0, "gesture mode");
                ram_path(argv[2], true);
                std::ifstream file(argv[2], std::ios::binary | std::ios::ate);
                require(bool(file), "native RAM state open");
                const auto length = file.tellg();
                require(length > 0 && length <= 64 * 1024 * 1024, "native state bound");
                session.saved.bytes.resize(size_t(length));
                file.seekg(0);
                require(bool(file.read(session.saved.bytes.data(), length)),
                        "native RAM state read");
                session.saved_valid = true;
                session.load_saved_state();
                session.saved_valid = false;
                session.perf = reinterpret_cast<bool (*)(const clap_plugin_t *, Perf *)>(
                    dlsym(module, "__kontra_clap_perf"));
                if (const auto *fd = std::getenv("KONTRA_WIDGET_RGB_FD")) {
                    const auto parsed = widget_host::parse(std::string("0 wait ") + fd).args[0];
                    require(parsed >= 3, "RGB descriptor number");
                    session.rgb_fd = parsed;
                    ram_fd(session.rgb_fd);
                }
                session.start_audio();
                session.await_ready();
                session.open_editor();
                std::printf("{\"schema\":1,\"kind\":\"ready\",\"generation\":%u,\"width\":%u,"
                            "\"height\":%u,\"load_ready\":%s}\n",
                            session.generation, session.width, session.height,
                            session.perf ? "true" : "false");
                std::fflush(stdout);
                session.plan();
            }
        }
        entry->deinit();
        dlclose(module);
        XCloseDisplay(display);
        return 0;
    } catch (const std::exception &error) {
        std::fprintf(stderr, "widget-host failure: %s\n", error.what());
        if (initialized)
            entry->deinit();
        if (module)
            dlclose(module);
        if (display)
            XCloseDisplay(display);
        return 1;
    }
}
