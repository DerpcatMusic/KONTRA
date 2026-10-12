// Actual shim lifecycle calls; no editor, library or substitute event adapter.
// g++ -std=c++17 vendor/moose-vst3/tests/lifecycle.cpp -o /tmp/kontra-lifecycle
#include "../shim/vst3_shim.cpp"
#include <cstring>

static int token, resets;
int main(int argc, char** argv) {
    Vst3Callbacks callbacks{};
    callbacks.create = []() -> void* { return &token; };
    callbacks.destroy = [](void*) {};
    callbacks.reset = [](void* ctx, double rate, uint32_t frames, int32_t mode) {
        if (ctx != &token || rate != 44100. || frames != 1024 || mode != 0) std::abort();
        ++resets;
    };
    callbacks.set_active = [](void*, int32_t) {};
    g_cb = &callbacks;
    MooseComponent component;
    component.setActive(1);
    if (resets != 1) return 1;
    if (argc == 2 && std::strcmp(argv[1], "processing") == 0) {
        component.setProcessing(1);
        if (resets != 1) return 2;
        component.setProcessing(0);
    } else {
        component.setActive(0);
    }
    if (resets != 2) return 3;
    std::puts("lifecycle_reset=PASS");
}
