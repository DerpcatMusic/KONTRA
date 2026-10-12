// Driver-free actual-shim regression (staged; run by the central validator):
// g++ -std=c++17 vendor/moose-vst3/tests/process_failure.cpp -o /tmp/kontra-process-failure
// /tmp/kontra-process-failure
#include "../shim/vst3_shim.cpp"

static int context_token, calls, output_probes;
static uint32_t outcome, finished;

int main() {
    Vst3PluginDescriptor descriptor{};
    descriptor.supports_f64 = 1;
    Vst3Callbacks callbacks{};
    callbacks.create = []() -> void* { return &context_token; };
    callbacks.destroy = [](void*) {};
    auto process = [](void*, auto, auto, uint32_t, uint32_t,
                      const uint32_t*, uint32_t, uint32_t,
                      const uint32_t*, uint32_t, uint32_t, uint32_t,
                      const Vst3Transport*, const Vst3ParamChange*, uint32_t, int32_t) -> uint32_t {
        ++calls;
        return outcome;
    };
    callbacks.process = process;
    callbacks.process_f64 = process;
    callbacks.get_output_param_count = [](void*) -> uint32_t { ++output_probes; return 0; };
    callbacks.finish_output_events = [](void*, uint32_t status) { finished = status; };
    g_desc = &descriptor;
    g_cb = &callbacks;
    MooseComponent component;

    for (int32 format = kSample32; format <= kSample64; ++format) {
        ProcessSetup setup{0, format, 8, 48000.0};
        if (component.setupProcessing(&setup) != kResultOk) return 1;
        ProcessData data{};
        data.numSamples = 8;
        data.symbolicSampleSize = format;
        const int before = calls;
        outcome = 0;
        finished = 99;
        output_probes = 0;
        if (component.process(&data) != kResultFalse || calls != before + 1
            || finished != 3 || output_probes != 0) return 2;
        outcome = 1;
        if (component.process(&data) != kResultOk || calls != before + 2
            || output_probes != 1) return 3;
    }
    std::puts("PASS: failed f32/f64 process returns kResultFalse without draining output");
}
