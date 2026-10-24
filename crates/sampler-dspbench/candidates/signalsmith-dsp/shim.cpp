// C ABI over Signalsmith DSP (MIT, see LICENSE.txt): the BiquadStatic bell/shelf designs and the
// Oversampler2xFIR half-band. Built by build.rs when the `candidates` feature is on.
#include "filters.h"
#include "rates.h"

using Biquad = signalsmith::filters::BiquadStatic<double>;
using Over = signalsmith::rates::Oversampler2xFIR<double>;
using Design = signalsmith::filters::BiquadDesign;

extern "C" {

void* sig_bq_new() { return new Biquad(); }
void sig_bq_free(void* p) { delete static_cast<Biquad*>(p); }

// shape: 0 peak, 1 low shelf, 2 high shelf. design: 0 vicanek, 1 cookbook, 2 bilinear, 3 one-sided.
// scaled_freq is f / fs; db is the section gain; q is the bandwidth.
void sig_bq_set(void* p, int shape, int design, double scaled_freq, double db, double q) {
    Design d = design == 0 ? Design::vicanek : design == 1 ? Design::cookbook : design == 2 ? Design::bilinear : Design::oneSided;
    auto* b = static_cast<Biquad*>(p);
    if (shape == 0) b->peakDbQ(scaled_freq, db, q, d);
    else if (shape == 1) b->lowShelfDbQ(scaled_freq, db, q, d);
    else b->highShelfDbQ(scaled_freq, db, q, d);
}

double sig_bq_tick(void* p, double x) { return (*static_cast<Biquad*>(p))(x); }
void sig_bq_reset(void* p) { static_cast<Biquad*>(p)->reset(); }

void* sig_os_new(int max_block) {
    auto* o = new Over();
    o->resize(1, max_block);
    return o;
}
void sig_os_free(void* p) { delete static_cast<Over*>(p); }
int sig_os_latency(void* p) { return static_cast<Over*>(p)->latency(); }
void sig_os_reset(void* p) { static_cast<Over*>(p)->reset(); }

// Upsample `n` input frames: the 2n-frame buffer is then at sig_os_buffer().
void sig_os_up(void* p, const double* in, int n) {
    const double* chans[1] = {in};
    static_cast<Over*>(p)->up(chans, n);
}
double* sig_os_buffer(void* p) { return (*static_cast<Over*>(p))[0]; }
// Downsample 2n frames of the buffer (after any processing) to `n` output frames.
void sig_os_down(void* p, double* out, int n) {
    double* chans[1] = {out};
    static_cast<Over*>(p)->down(chans, n);
}

}
