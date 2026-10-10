// Synthetic CLAP fixture: actual X11 events own parameter 77; state restores it.
// No instrument, library data, product workaround or frozen-v1 code is involved.
#include <X11/Xlib.h>
#include <X11/keysym.h>
#include <algorithm>
#include <atomic>
#include <clap/clap.h>
#include <cstdio>
#include <cstring>
static const char *features[] = {CLAP_PLUGIN_FEATURE_INSTRUMENT, nullptr};
static const clap_plugin_descriptor_t descriptor{CLAP_VERSION,
                                                 "kontra.widget-transport-fixture",
                                                 "Widget transport fixture",
                                                 "KONTRA tests",
                                                 "",
                                                 "",
                                                 "",
                                                 "1",
                                                 "Synthetic OS-input and state fixture",
                                                 features};
static std::atomic<unsigned> instances{0};
struct Fixture {
    clap_plugin_t plugin{};
    const clap_host_t *host;
    std::atomic<double> value{0.25};
    unsigned instance = ++instances;
    Display *display = nullptr;
    Window child = 0;
    uint32_t width = 200, height = 100;
    bool gui_created = false;
    explicit Fixture(const clap_host_t *h) : host(h) {}
    static Fixture &from(const clap_plugin_t *p) { return *static_cast<Fixture *>(p->plugin_data); }
    void repaint() {
        if (child) {
            XSetWindowBackground(display, child, value >= 0.5 ? 0x559944 : 0x112233);
            XClearWindow(display, child);
            XSync(display, False);
        }
    }
    static const void *extension(const clap_plugin_t *, const char *);
    void events() {
        if (!display)
            return;
        while (XPending(display)) {
            XEvent event{};
            XNextEvent(display, &event);
            if (event.type == ButtonPress && event.xbutton.button == 1) {
                value = 0.75;
                repaint();
            }
            if (event.type == KeyPress && XLookupKeysym(&event.xkey, 0) == XK_space) {
                value = 0.5;
                repaint();
            }
        }
    }
};
static clap_plugin_state_t state{[](const clap_plugin_t *p, const clap_ostream_t *stream) {
                                     const auto v = Fixture::from(p).value.load();
                                     return stream->write(stream, &v, sizeof(v)) == sizeof(v);
                                 },
                                 [](const clap_plugin_t *p, const clap_istream_t *stream) {
                                     double v;
                                     if (stream->read(stream, &v, sizeof(v)) != sizeof(v))
                                         return false;
                                     Fixture::from(p).value = v;
                                     return true;
                                 }};
static clap_plugin_params_t params{
    [](const clap_plugin_t *) -> uint32_t { return 2; },
    [](const clap_plugin_t *, uint32_t at, clap_param_info_t *out) {
        if (at >= 2)
            return false;
        *out = {};
        out->id = at ? 78 : 77;
        out->flags = at ? CLAP_PARAM_IS_READONLY : CLAP_PARAM_IS_AUTOMATABLE;
        std::strcpy(out->name, at ? "Created instance" : "OS-controlled value");
        out->min_value = 0;
        out->max_value = at ? 4096 : 1;
        out->default_value = at ? 1 : 0.25;
        return true;
    },
    [](const clap_plugin_t *p, clap_id id, double *out) {
        if (id != 77 && id != 78)
            return false;
        auto &f = Fixture::from(p);
        *out = id == 77 ? f.value.load() : f.instance;
        return true;
    },
    [](const clap_plugin_t *, clap_id, double value, char *out, uint32_t capacity) {
        return std::snprintf(out, capacity, "%.3f", value) >= 0;
    },
    [](const clap_plugin_t *, clap_id, const char *, double *) { return false; },
    [](const clap_plugin_t *, const clap_input_events_t *, const clap_output_events_t *) {}};
static clap_plugin_audio_ports_t ports{
    [](const clap_plugin_t *, bool input) -> uint32_t { return input ? 0 : 1; },
    [](const clap_plugin_t *, uint32_t at, bool input, clap_audio_port_info_t *out) {
        if (input || at)
            return false;
        *out = {};
        out->id = 0;
        out->flags = CLAP_AUDIO_PORT_IS_MAIN;
        std::strcpy(out->name, "Fixture");
        out->channel_count = 2;
        out->port_type = CLAP_PORT_STEREO;
        out->in_place_pair = CLAP_INVALID_ID;
        return true;
    }};
static clap_plugin_gui_t gui{
    [](const clap_plugin_t *, const char *api, bool floating) {
        return !floating && api && std::strcmp(api, CLAP_WINDOW_API_X11) == 0;
    },
    [](const clap_plugin_t *, const char **api, bool *floating) {
        *api = CLAP_WINDOW_API_X11;
        *floating = false;
        return true;
    },
    [](const clap_plugin_t *p, const char *, bool) {
        auto &f = Fixture::from(p);
        f.display = XOpenDisplay(nullptr);
        f.gui_created = bool(f.display);
        return f.gui_created;
    },
    [](const clap_plugin_t *p) {
        auto &f = Fixture::from(p);
        if (f.child)
            XDestroyWindow(f.display, f.child);
        if (f.display)
            XCloseDisplay(f.display);
        f.child = 0;
        f.display = nullptr;
        f.gui_created = false;
    },
    [](const clap_plugin_t *, double) { return true; },
    [](const clap_plugin_t *p, uint32_t *w, uint32_t *h) {
        auto &f = Fixture::from(p);
        *w = f.width;
        *h = f.height;
        return true;
    },
    [](const clap_plugin_t *) { return true; },
    [](const clap_plugin_t *, clap_gui_resize_hints_t *hints) {
        *hints = {};
        hints->can_resize_horizontally = hints->can_resize_vertically = true;
        return true;
    },
    [](const clap_plugin_t *, uint32_t *w, uint32_t *h) {
        return *w > 0 && *h > 0 && *w <= 4096 && *h <= 2160;
    },
    [](const clap_plugin_t *p, uint32_t w, uint32_t h) {
        if (!w || !h || w > 4096 || h > 2160)
            return false;
        auto &f = Fixture::from(p);
        f.width = w;
        f.height = h;
        if (f.child)
            XResizeWindow(f.display, f.child, w, h);
        return true;
    },
    [](const clap_plugin_t *p, const clap_window_t *parent) {
        auto &f = Fixture::from(p);
        if (!f.display || std::strcmp(parent->api, CLAP_WINDOW_API_X11) != 0)
            return false;
        f.child =
            XCreateSimpleWindow(f.display, parent->x11, 0, 0, f.width, f.height, 0, 0, 0x112233);
        XSelectInput(f.display, f.child,
                     ButtonPressMask | ButtonReleaseMask | KeyPressMask | KeyReleaseMask |
                         PointerMotionMask | ExposureMask);
        XSync(f.display, False);
        return true;
    },
    [](const clap_plugin_t *, const clap_window_t *) { return false; },
    [](const clap_plugin_t *, const char *) {},
    [](const clap_plugin_t *p) {
        auto &f = Fixture::from(p);
        XMapWindow(f.display, f.child);
        f.repaint();
        return true;
    },
    [](const clap_plugin_t *p) {
        auto &f = Fixture::from(p);
        XUnmapWindow(f.display, f.child);
        XSync(f.display, False);
        return true;
    }};
const void *Fixture::extension(const clap_plugin_t *, const char *id) {
    if (std::strcmp(id, CLAP_EXT_STATE) == 0)
        return &state;
    if (std::strcmp(id, CLAP_EXT_PARAMS) == 0)
        return &params;
    if (std::strcmp(id, CLAP_EXT_AUDIO_PORTS) == 0)
        return &ports;
    return std::strcmp(id, CLAP_EXT_GUI) == 0 ? &gui : nullptr;
}
static clap_plugin_factory_t factory{
    [](const clap_plugin_factory_t *) -> uint32_t { return 1; },
    [](const clap_plugin_factory_t *, uint32_t at) { return at ? nullptr : &descriptor; },
    [](const clap_plugin_factory_t *, const clap_host_t *host,
       const char *id) -> const clap_plugin_t * {
        if (std::strcmp(id, descriptor.id) != 0)
            return nullptr;
        auto *f = new Fixture(host);
        f->plugin = {
            &descriptor,
            f,
            [](const clap_plugin_t *) { return true; },
            [](const clap_plugin_t *p) {
                auto &f = Fixture::from(p);
                if (f.gui_created)
                    gui.destroy(p);
                delete &f;
            },
            [](const clap_plugin_t *, double, uint32_t, uint32_t) { return true; },
            [](const clap_plugin_t *) {},
            [](const clap_plugin_t *) { return true; },
            [](const clap_plugin_t *) {},
            [](const clap_plugin_t *) {},
            [](const clap_plugin_t *p, const clap_process_t *process) -> clap_process_status {
                auto &f = Fixture::from(p);
                for (uint32_t b = 0; b < process->audio_outputs_count; ++b) {
                    auto &output = process->audio_outputs[b];
                    output.constant_mask = (uint64_t(1) << output.channel_count) - 1;
                    for (uint32_t c = 0; c < output.channel_count; ++c)
                        output.data32[c][0] = f.value.load();
                }
                f.host->request_callback(f.host);
                return CLAP_PROCESS_CONTINUE;
            },
            Fixture::extension,
            [](const clap_plugin_t *p) { Fixture::from(p).events(); }};
        return &f->plugin;
    }};
extern "C" CLAP_EXPORT const clap_plugin_entry_t clap_entry{
    CLAP_VERSION, [](const char *) { return true; }, [] {},
    [](const char *id) -> const void * {
        return std::strcmp(id, CLAP_PLUGIN_FACTORY_ID) == 0 ? &factory : nullptr;
    }};
extern "C" CLAP_EXPORT bool __kontra_clap_perf(const clap_plugin_t *, void *data) {
    auto *fields = static_cast<uint64_t *>(data);
    std::fill(fields, fields + 11, 0);
    fields[9] = 1;
    return true;
}
