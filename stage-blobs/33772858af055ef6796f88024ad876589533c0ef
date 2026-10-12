// Driver-free actual-shim regression:
// g++ -std=c++17 vendor/moose-vst3/tests/editor_attach.cpp -o /tmp/kontra-editor-attach
// /tmp/kontra-editor-attach
#include "../shim/vst3_shim.cpp"

static int context_token;
static int opens, closes, restores, resets;
static void* observed_parent;
static uint32_t width = 1180, height = 760;

struct StateStream { IBStreamVtbl* vtbl; bool read = false; };

int main() {
    Vst3Callbacks callbacks{};
    callbacks.create = []() -> void* { return &context_token; };
    callbacks.destroy = [](void*) {};
    callbacks.gui_open = [](void* context, void* parent) {
        if (context != &context_token) std::abort();
        ++opens;
        observed_parent = parent;
    };
    callbacks.gui_close = [](void*) { ++closes; };
    callbacks.gui_get_size = [](void*, uint32_t* w, uint32_t* h) { *w = width; *h = height; };
    callbacks.gui_set_size = [](void*, uint32_t w, uint32_t h) { width = w; height = h; };
    callbacks.state_load = [](void*, const uint8_t*, uint32_t) -> int32_t { ++restores; return 1; };
    callbacks.reset = [](void*, double, uint32_t, int32_t) { ++resets; };
    callbacks.set_active = [](void*, int32_t) {};
    g_cb = &callbacks;
    MooseComponent component;
    MoosePlugView view{};
    view.vtbl = &g_plugview_vtbl;
    view.ctx = component.rustContext();
    view.comp = &component;
    int parent_token;
    void* parent = &parent_token; // Only forwarded to fake callbacks, never dereferenced.
    #if defined(__APPLE__)
    const auto platform = kPlatformTypeNSView;
    #elif defined(_WIN32)
    const auto platform = kPlatformTypeHWND;
    #else
    const auto platform = kPlatformTypeX11;
    #endif

    // Reject native representation mismatches before changing attachment
    // state or forwarding the pointer to Rust/native code.
    const char* rejected_types[] = {"unsupported", "HIView", "UIView", "nsview", nullptr};
    for (const char* type : rejected_types) {
        if (view.vtbl->attached(&view, parent, type) != kResultFalse
            || component.attachedView || opens || closes) return 10;
    }
    if (view.vtbl->isPlatformTypeSupported(&view, nullptr) != kResultFalse
        || view.vtbl->isPlatformTypeSupported(&view, platform) != kResultOk) return 11;
    if (view.vtbl->attached(&view, nullptr, platform) != kResultFalse
        || component.attachedView || opens || closes) return 12;
    callbacks.gui_open = nullptr;
    if (view.vtbl->attached(&view, parent, platform) != kResultFalse
        || component.attachedView || opens || closes) return 13;
    callbacks.gui_open = [](void* context, void* parent) {
        if (context != &context_token) std::abort();
        ++opens;
        observed_parent = parent;
    };

    if (view.vtbl->removed(&view) != kResultOk || closes != 0) return 15;

    // Fresh inactive instance: no state restore or audio activation precedes
    // IPlugView::attached. The editor must exist when attached succeeds.
    if (view.vtbl->attached(&view, parent, platform) != kResultOk
        || opens != 1 || observed_parent != parent) {
        std::fprintf(stderr, "FAIL: attached fresh inactive view, gui_open calls=%d\n", opens);
        return 1;
    }
    // A failed reattachment must preserve the currently attached editor.
    if (view.vtbl->attached(&view, parent, "unsupported") != kResultFalse
        || component.attachedView != &view || opens != 1 || closes != 0) return 14;

    ViewRect size{};
    if (view.vtbl->getSize(&view, &size) != kResultOk || size.right != 1180 || size.bottom != 760) return 8;
    size = {10, 20, 1310, 920};
    if (view.vtbl->onSize(&view, &size) != kResultOk || width != 1300 || height != 900) return 9;

    IBStreamVtbl stream_vtbl{};
    stream_vtbl.read = [](void* s, void* out, int32, int32* count) -> tresult {
        auto* stream = static_cast<StateStream*>(s);
        *count = stream->read ? 0 : 1;
        if (*count) *static_cast<uint8_t*>(out) = 0;
        stream->read = true;
        return kResultOk;
    };
    StateStream stream{&stream_vtbl};
    if (component.setState(&stream) != kResultOk || restores != 1 || opens != 1) return 2;
    if (component.setActive(1) != kResultOk || resets != 1 || opens != 1) return 3;
    if (view.vtbl->removed(&view) != kResultOk || closes != 1) return 4;
    if (component.setActive(0) != kResultOk || opens != 1) return 5;

    // A reopened editor remains independent of processor activation.
    if (view.vtbl->attached(&view, parent, platform) != kResultOk || opens != 2) return 6;
    if (view.vtbl->removed(&view) != kResultOk || closes != 2) return 7;
    std::puts("PASS: native parent validation, rejected attachment state, inactive attach, resize, restore/activation, remove and reopen");
}
