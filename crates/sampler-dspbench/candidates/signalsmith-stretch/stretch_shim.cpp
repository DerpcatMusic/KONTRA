// C entry points for Signalsmith Stretch (MIT, Copyright 2022 Geraint Luff), mono. Bench only.
#include "signalsmith-stretch.h"

using Stretch = signalsmith::stretch::SignalsmithStretch<float>;

extern "C" {
void* sst_new(int rate) {
    auto* s = new Stretch();
    s->presetDefault(1, float(rate));
    return s;
}
void sst_free(void* p) { delete static_cast<Stretch*>(p); }
void sst_reset(void* p) { static_cast<Stretch*>(p)->reset(); }
int sst_output_latency(void* p) { return static_cast<Stretch*>(p)->outputLatency(); }
// Consume `in_n` input frames and produce `out_n` output frames (mono).
void sst_process(void* p, const float* in, int in_n, float* out, int out_n) {
    const float* ins[1] = {in};
    float* outs[1] = {out};
    static_cast<Stretch*>(p)->process(ins, in_n, outs, out_n);
}
}
