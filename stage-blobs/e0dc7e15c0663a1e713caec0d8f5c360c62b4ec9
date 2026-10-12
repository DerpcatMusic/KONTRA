// Real embedded CLAP GUI. Stdout is RAM-only RGB frames, never a log/file.
// HOST PLUGIN NATIVE_STATE FRAMES; compile with official CLAP headers, -lX11 -ldl -pthread.
#include <clap/clap.h>
#include <X11/Xlib.h>
#include <X11/Xutil.h>
#include <X11/Xatom.h>
#include <X11/extensions/XTest.h>
#include <dlfcn.h>
#include <algorithm>
#include <array>
#include <atomic>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <thread>
#include <vector>
#include <unistd.h>

using Clock = std::chrono::steady_clock;
static thread_local bool audio_thread = false;
static void require(bool ok, const char* stage) {
    if (!ok) { std::fprintf(stderr, "presented-host failure: %s\n", stage); std::exit(1); }
}
struct Stream {
    std::vector<char> bytes; size_t cursor = 0;
    clap_istream_t api{this, [](const clap_istream_t* in, void* out, uint64_t size)->int64_t {
        auto& s = *static_cast<Stream*>(in->ctx);
        auto n = std::min<uint64_t>(size, s.bytes.size()-s.cursor);
        std::memcpy(out, s.bytes.data()+s.cursor, n); s.cursor += n; return n;
    }};
};
struct Host {
    std::thread::id main = std::this_thread::get_id();
    std::atomic<bool> callback{false};
    std::atomic<uint64_t> resize{0};
    clap_host_gui_t gui{[](const clap_host_t*) {},
        [](const clap_host_t* h, uint32_t w, uint32_t height) {
            if (!w || !height || w > 4096 || height > 2160) return false;
            static_cast<Host*>(h->host_data)->resize = (uint64_t(w)<<32)|height; return true;
        }, [](const clap_host_t*) { return true; }, [](const clap_host_t*) { return false; }, [](const clap_host_t*, bool) {}};
    clap_host_thread_check_t threads{
        [](const clap_host_t* h) { return std::this_thread::get_id() == static_cast<Host*>(h->host_data)->main; },
        [](const clap_host_t*) { return audio_thread; }};
    clap_host_t api{CLAP_VERSION, this, "KONTRA presented gate", "KONTRA", "", "1",
        [](const clap_host_t* h, const char* id)->const void* {
            auto& s = *static_cast<Host*>(h->host_data);
            if (std::strcmp(id, CLAP_EXT_THREAD_CHECK) == 0) return &s.threads;
            return std::strcmp(id, CLAP_EXT_GUI) == 0 ? &s.gui : nullptr;
        }, [](const clap_host_t*) {}, [](const clap_host_t*) {},
        [](const clap_host_t* h) { static_cast<Host*>(h->host_data)->callback = true; }};
};
struct Perf { uint64_t busy, span, voices, audible, dropouts, memory, freed, disk, underruns, loaded, blocks; };
int main(int argc, char** argv) {
    require(argc == 4, "PLUGIN STATE FRAMES");
    const int count = std::atoi(argv[3]); require(count >= 5 && count <= 120, "frame bound");
    require(XInitThreads(), "X11 threads");
    auto* display = XOpenDisplay(nullptr); require(display, "X11 display");
    void* module = dlopen(argv[1], RTLD_NOW|RTLD_LOCAL); require(module, "module");
    auto* entry = static_cast<const clap_plugin_entry_t*>(dlsym(module, "clap_entry"));
    require(entry && entry->init(argv[1]), "entry");
    auto* factory = static_cast<const clap_plugin_factory_t*>(entry->get_factory(CLAP_PLUGIN_FACTORY_ID));
    require(factory, "factory"); auto* descriptor = factory->get_plugin_descriptor(factory, 0); require(descriptor, "descriptor");
    Host host; auto* p = factory->create_plugin(factory, &host.api, descriptor->id); require(p && p->init(p), "init");
    std::ifstream file(argv[2], std::ios::binary);
    Stream state{{std::istreambuf_iterator<char>(file), std::istreambuf_iterator<char>()}};
    auto* save = static_cast<const clap_plugin_state_t*>(p->get_extension(p, CLAP_EXT_STATE));
    require(save && !state.bytes.empty() && save->load(p, &state.api), "state load");
    auto* ports = static_cast<const clap_plugin_audio_ports_t*>(p->get_extension(p, CLAP_EXT_AUDIO_PORTS));
    require(ports, "audio ports"); const auto buses = ports->count(p, false); require(buses > 0 && buses <= 16, "bus bound");
    std::vector<std::vector<std::array<float, 64>>> pcm(buses);
    std::vector<std::vector<float*>> pointers(buses); std::vector<clap_audio_buffer_t> outputs(buses);
    for (uint32_t b=0; b<buses; ++b) {
        clap_audio_port_info_t info{}; require(ports->get(p, b, false, &info) && info.channel_count > 0 && info.channel_count <= 16, "channel bound");
        pcm[b].resize(info.channel_count); for (auto& channel : pcm[b]) pointers[b].push_back(channel.data());
        outputs[b] = {pointers[b].data(), nullptr, info.channel_count, 0, 0};
    }
    clap_input_events_t in{nullptr, [](const clap_input_events_t*)->uint32_t { return 0; },
        [](const clap_input_events_t*, uint32_t)->const clap_event_header_t* { return nullptr; }};
    clap_output_events_t out{nullptr, [](const clap_output_events_t*, const clap_event_header_t*) { return true; }};
    require(p->activate(p, 48000, 64, 64), "activate");
    std::atomic<bool> run{true}, started{false};
    std::thread audio([&] {
        audio_thread = true; require(p->start_processing(p), "start"); started = true;
        clap_process_t process{}; process.frames_count=64; process.audio_outputs=outputs.data();
        process.audio_outputs_count=buses; process.in_events=&in; process.out_events=&out;
        auto next = Clock::now();
        while (run) {
            require(p->process(p, &process) != CLAP_PROCESS_ERROR, "process"); process.steady_time += 64;
            next += std::chrono::nanoseconds(1333333); std::this_thread::sleep_until(next);
        }
        p->stop_processing(p);
    });
    auto perf = reinterpret_cast<bool (*)(const clap_plugin_t*, Perf*)>(dlsym(module, "__kontra_clap_perf"));
    require(perf, "readiness export");
    const auto deadline = Clock::now()+std::chrono::seconds(120);
    Perf metrics{};
    while (!started || !perf(p, &metrics) || metrics.loaded != 1) {
        require(Clock::now() < deadline, "load deadline");
        if (host.callback.exchange(false)) p->on_main_thread(p);
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }
    auto* gui = static_cast<const clap_plugin_gui_t*>(p->get_extension(p, CLAP_EXT_GUI));
    require(gui && gui->is_api_supported(p, CLAP_WINDOW_API_X11, false) && gui->create(p, CLAP_WINDOW_API_X11, false), "GUI create");
    uint32_t width=0, height=0; require(gui->get_size(p, &width, &height), "GUI size");
    require(width > 0 && height > 0 && width <= 4096 && height <= 2160, "GUI bounds");
    const auto window = XCreateSimpleWindow(display, DefaultRootWindow(display), 20, 20, width, height, 0, 0, 0);
    const unsigned long pid = getpid();
    XChangeProperty(display, window, XInternAtom(display, "_NET_WM_PID", False), XA_CARDINAL, 32, PropModeReplace,
        reinterpret_cast<const unsigned char*>(&pid), 1);
    XSelectInput(display, window, StructureNotifyMask);
    XStoreName(display, window, "KONTRA presented gate"); XMapRaised(display, window); XSync(display, False);
    clap_window_t parent{}; parent.api=CLAP_WINDOW_API_X11; parent.x11=window;
    require(gui->set_parent(p, &parent) && gui->show(p), "GUI attach/show");
    std::fprintf(stderr, "presented window=%lu size=%ux%u\n", window, width, height);
    const auto beginning = Clock::now();
    for (int index=-20; index<count; ++index) {
        if (host.callback.exchange(false)) p->on_main_thread(p);
        while (XPending(display)) {
            XEvent event{}; XNextEvent(display, &event);
            if (event.type == ConfigureNotify && event.xconfigure.window == window)
                require(gui->set_size(p, event.xconfigure.width, event.xconfigure.height), "host resize");
        }
        if (const auto size=host.resize.exchange(0)) XResizeWindow(display, window, size>>32, size&0xffffffff);
        std::this_thread::sleep_until(beginning+std::chrono::milliseconds((index+21)*100));
        if (index < 0) continue;
        // Parent pixels exclude redirected child surfaces under Xwayland.
        Window root=0, ignored=0, *children=nullptr; unsigned child_count=0;
        require(XQueryTree(display, window, &root, &ignored, &children, &child_count), "child query");
        require(child_count == 1, "one embedded editor child");
        const auto surface=children[0]; XFree(children);
        XWindowAttributes attributes{}; require(XGetWindowAttributes(display, surface, &attributes), "child attributes");
        require(attributes.map_state == IsViewable && attributes.width > 0 && attributes.height > 0
            && attributes.width <= 4096 && attributes.height <= 2160, "visible editor child");
        width=attributes.width; height=attributes.height;
        if (index == 0) {
            int x=0, y=0; Window child=0;
            require(XTranslateCoordinates(display, surface, DefaultRootWindow(display), 0, 0, &x, &y, &child), "pointer coordinates");
            // Arm existing native timing with a two-pixel primary drag on the resize grip.
            XTestFakeMotionEvent(display, -1, x+int(width)-10, y+int(height)-10, CurrentTime);
            XTestFakeButtonEvent(display, 1, True, CurrentTime);
            XTestFakeMotionEvent(display, -1, x+int(width)-12, y+int(height)-12, CurrentTime);
            XTestFakeButtonEvent(display, 1, False, CurrentTime); XFlush(display);
        }
        auto* image = XGetImage(display, surface, 0, 0, width, height, AllPlanes, ZPixmap); require(image, "presented readback");
        const uint64_t ns = std::chrono::duration_cast<std::chrono::nanoseconds>(Clock::now().time_since_epoch()).count();
        std::fwrite(&width, 4, 1, stdout); std::fwrite(&height, 4, 1, stdout); std::fwrite(&ns, 8, 1, stdout);
        std::vector<unsigned char> rgb(width*height*3);
        require(image->red_mask == 0xff0000 && image->green_mask == 0xff00 && image->blue_mask == 0xff, "RGB visual masks");
        for (unsigned y=0; y<height; ++y) for (unsigned x=0; x<width; ++x) {
            const auto pixel=XGetPixel(image, x, y); auto* value=&rgb[(y*width+x)*3];
            value[0]=(pixel>>16)&255; value[1]=(pixel>>8)&255; value[2]=pixel&255;
        }
        std::fwrite(rgb.data(), 1, rgb.size(), stdout); std::fflush(stdout); XDestroyImage(image);
    }
    gui->destroy(p); run=false; audio.join(); p->deactivate(p); p->destroy(p);
    entry->deinit(); dlclose(module); XDestroyWindow(display, window); XCloseDisplay(display);
}
