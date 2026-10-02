/**
 * Moose VST3 C++ shim.
 *
 * Implements the VST3 COM interfaces (IComponent, IAudioProcessor,
 * IEditController, IPluginFactory) in real C++ so the vtable layout
 * matches what hosts expect. All plugin logic is delegated to Rust
 * via C FFI callbacks.
 */

#include <cstdint>
#include <cstddef>
#include <cstdio>
#include <cstring>
#include <cstdlib>
#include <cmath>
#include <atomic>
#include <new>
#include <vector>
#if __APPLE__
// libdispatch: an event-driven bridge that marshals a pending
// restartComponent onto the host's main run loop, so a latency change
// notifies the host even with no editor open and no host param polling
// (the one case the main-thread callback hooks below can't reach).
#include <dispatch/dispatch.h>
#endif
// Linux uses the host's Steinberg::Linux::IRunLoop (queried off the plug
// frame) with a periodic ITimerHandler to drain a pending restart on the
// host UI thread - see the run-loop bridge below. It deliberately makes
// no eventfd / self-pipe syscall: Bitwig instantiates plugins in a
// seccomp-sandboxed process where a stray syscall is fatal, and a timer
// needs none. Linux has no process-global main queue, so the no-editor
// path falls back to the param-poll / edit-gesture flush.
#if defined(_WIN32)
// Windows uses a null-HWND thread timer (SetTimer) created on the
// instantiating thread: WM_TIMER is dispatched by that thread's message
// pump, so the TIMERPROC drains a pending restart on the host UI thread
// for the component's whole life - editor or not - like the macOS
// dispatch source. Message queues are per-thread on Windows, so no
// plug-frame run loop is needed.
#define WIN32_LEAN_AND_MEAN
// mingw's headers predefine NOMINMAX; guard so a cross build stays
// warning-free.
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#endif

#define I(x) static_cast<int8_t>(x)

// VST3 IIDs have different byte order on Windows (COM-compatible) vs
// macOS/Linux. This macro produces the right 16-byte layout per platform.
// Call with 4 uint32 values that together represent the UUID's natural form.
#if defined(_WIN32)
#define MAKE_IID(l1, l2, l3, l4) { \
    I((l1) & 0xFF), I(((l1) >> 8) & 0xFF), I(((l1) >> 16) & 0xFF), I(((l1) >> 24) & 0xFF), \
    I(((l2) >> 16) & 0xFF), I(((l2) >> 24) & 0xFF), I((l2) & 0xFF), I(((l2) >> 8) & 0xFF), \
    I(((l3) >> 24) & 0xFF), I(((l3) >> 16) & 0xFF), I(((l3) >> 8) & 0xFF), I((l3) & 0xFF), \
    I(((l4) >> 24) & 0xFF), I(((l4) >> 16) & 0xFF), I(((l4) >> 8) & 0xFF), I((l4) & 0xFF) \
}
#else
#define MAKE_IID(l1, l2, l3, l4) { \
    I(((l1) >> 24) & 0xFF), I(((l1) >> 16) & 0xFF), I(((l1) >> 8) & 0xFF), I((l1) & 0xFF), \
    I(((l2) >> 24) & 0xFF), I(((l2) >> 16) & 0xFF), I(((l2) >> 8) & 0xFF), I((l2) & 0xFF), \
    I(((l3) >> 24) & 0xFF), I(((l3) >> 16) & 0xFF), I(((l3) >> 8) & 0xFF), I((l3) & 0xFF), \
    I(((l4) >> 24) & 0xFF), I(((l4) >> 16) & 0xFF), I(((l4) >> 8) & 0xFF), I((l4) & 0xFF) \
}
#endif

// ---------------------------------------------------------------------------
// Minimal VST3 type definitions (no SDK dependency)
// ---------------------------------------------------------------------------

typedef int32_t tresult;
typedef int32_t int32;
typedef uint32_t uint32;
typedef int16_t char16;
typedef int8_t int8;
typedef char char8;
typedef int8_t TBool;
typedef uint64_t TSize;

typedef int8_t TUID[16];
typedef const char* FIDString;

// Per-direction channel-pointer capacity in process() (fixed - it's the
// audio callback). Registration rejects any plugin whose widest declared
// layout exceeds this, so an accepted descriptor never overruns or
// truncates. Keep in step with `VST3_MAX_CHANNELS_PER_DIRECTION` in lib.rs.
static constexpr uint32_t kMaxProcChannels = 32;

// tresult result codes. Like the IIDs above, the values differ between
// the Windows COM (HRESULT) ABI and the sequential enum every other
// platform uses - guard them the same way MAKE_IID is guarded. Getting
// these wrong makes a host misread codes: the old shim had
// kInvalidArgument=5 (the SDK's kNotInitialized) and kNotImplemented=2
// (the SDK's kInvalidArgument), and defined no kNoInterface. On Windows
// the small positive values are worse - SUCCEEDED(hr) treats any hr >= 0
// as success, so a "not implemented" or a partial-getState failure would
// read as OK and the host could consume an unwritten out-param.
#if defined(_WIN32)
static const tresult kNoInterface     = (tresult)0x80004002L; // E_NOINTERFACE
static const tresult kResultOk        = (tresult)0x00000000L; // S_OK
static const tresult kResultFalse     = (tresult)0x00000001L; // S_FALSE
static const tresult kInvalidArgument = (tresult)0x80070057L; // E_INVALIDARG
static const tresult kNotImplemented  = (tresult)0x80004001L; // E_NOTIMPL
#else
static const tresult kNoInterface     = -1;
static const tresult kResultOk        = 0;
static const tresult kResultFalse     = 1;
static const tresult kInvalidArgument = 2;
static const tresult kNotImplemented  = 3;
#endif

// IIDs from the official Steinberg VST3 SDK (via vst3 crate bindings).
// Byte layout differs between Windows (COM) and macOS/Linux - MAKE_IID handles this.
static const TUID FUnknown_iid        = MAKE_IID(0x00000000, 0x00000000, 0xC0000000, 0x00000046);
static const TUID IPluginBase_iid     = MAKE_IID(0x22888DDB, 0x156E45AE, 0x8358B348, 0x08190625);
static const TUID IComponent_iid      = MAKE_IID(0xE831FF31, 0xF2D54301, 0x928EBBEE, 0x25697802);
static const TUID IAudioProcessor_iid = MAKE_IID(0x42043F99, 0xB7DA453C, 0xA569E79D, 0x9AAEC33D);
static const TUID IEditController_iid = MAKE_IID(0xDCD7BBE3, 0x7742448D, 0xA874AACC, 0x979C759E);
static const TUID IPluginFactory_iid  = MAKE_IID(0x7A4D811C, 0x52114A1F, 0xAED9D2EE, 0x0B43BF9F);
static const TUID IPlugView_iid       = MAKE_IID(0x5BC32507, 0xD06049EA, 0xA6151B52, 0x2B755B29);
static const TUID IEditControllerHostEditing_iid = MAKE_IID(0xC1271208, 0x70594098, 0xB9DD34B3, 0x6BB0195E);
static const TUID IMidiMapping_iid = MAKE_IID(0xDF0FF9F7, 0x49B74669, 0xB63AB732, 0x7ADBF5E5);
static const TUID IProcessContextRequirements_iid = MAKE_IID(0x2A654303, 0xEF764E3D, 0x95B5FE83, 0x730EF6D0);
static const TUID IUnitInfo_iid       = MAKE_IID(0x3D4BD6B5, 0x913A4FD2, 0xA886E768, 0xA5EB92C1);
static const TUID INoteExpressionController_iid =
    MAKE_IID(0xB7F8F859, 0x41234872, 0x91169581, 0x4F3721A3);
static const TUID IPlugViewContentScaleSupport_iid =
    MAKE_IID(0x65ED9690, 0x8AC44525, 0x8AADEF7A, 0x72EA703F);
#if defined(__linux__)
// Steinberg::Linux run-loop interfaces: IRunLoop is the host's UI-thread
// event loop (queried off the plug frame); ITimerHandler is the periodic
// callback we register with it. Only needed on Linux.
static const TUID IRunLoop_iid      = MAKE_IID(0x18C35366, 0x97764F1A, 0x9C5B8385, 0x7A871389);
static const TUID ITimerHandler_iid = MAKE_IID(0x10BDD94F, 0x41424774, 0x821FAD8F, 0xECA72CA9);
#endif

// Only one of these matches the current build target; the others
// stay defined so `pv_isPlatformTypeSupported` reads uniformly.
[[maybe_unused]] static const char* kPlatformTypeNSView = "NSView";
[[maybe_unused]] static const char* kPlatformTypeHWND   = "HWND";
[[maybe_unused]] static const char* kPlatformTypeX11    = "X11EmbedWindowID";

// Media types & bus directions
enum { kAudio = 0, kEvent = 1 };
enum { kInput = 0, kOutput = 1 };
enum { kMain = 0, kAux = 1 };
enum { kSample32 = 0, kSample64 = 1 };

static bool iid_equal(const TUID a, const TUID b) {
    return memcmp(a, b, 16) == 0;
}

// UTF-8 -> UTF-16. A byte-per-unit widen sign-extends non-ASCII (a
// continuation byte 0x82 becomes 0xFF82 on platforms with signed `char`)
// and splits every multi-byte sequence into garbage units, so param
// titles/units ("µs", "±3 dB"), value strings, and bus/vendor names come
// out mojibake in every host. Decode UTF-8 and emit UTF-16 code units,
// surrogate pairs for astral code points. Index-based `(unsigned char)`
// reads (no pointer reinterpret_cast) keep this usable in constant
// expressions. `maxLen` bounds the output in char16 units, incl. the NUL.
static constexpr void str_to_char16(char16* dst, const char* src, int maxLen) {
    int i = 0;
    int o = 0;
    while (src[i] != 0 && o < maxLen - 1) {
        unsigned int b0 = (unsigned char)src[i];
        unsigned int b1 = (unsigned char)src[i + 1];
        unsigned int cp = 0;
        if (b0 < 0x80) {
            cp = b0;
            i += 1;
        } else if ((b0 & 0xE0) == 0xC0 && (b1 & 0xC0) == 0x80) {
            cp = ((b0 & 0x1F) << 6) | (b1 & 0x3F);
            i += 2;
        } else if ((b0 & 0xF0) == 0xE0 && (b1 & 0xC0) == 0x80
                   && ((unsigned char)src[i + 2] & 0xC0) == 0x80) {
            unsigned int b2 = (unsigned char)src[i + 2];
            cp = ((b0 & 0x0F) << 12) | ((b1 & 0x3F) << 6) | (b2 & 0x3F);
            i += 3;
        } else if ((b0 & 0xF8) == 0xF0 && (b1 & 0xC0) == 0x80
                   && ((unsigned char)src[i + 2] & 0xC0) == 0x80
                   && ((unsigned char)src[i + 3] & 0xC0) == 0x80) {
            unsigned int b2 = (unsigned char)src[i + 2];
            unsigned int b3 = (unsigned char)src[i + 3];
            cp = ((b0 & 0x07) << 18) | ((b1 & 0x3F) << 12) | ((b2 & 0x3F) << 6) | (b3 & 0x3F);
            i += 4;
        } else {
            // Invalid lead or truncated sequence: emit U+FFFD and resync
            // one byte, so malformed input can't loop or over-read.
            cp = 0xFFFD;
            i += 1;
        }
        if (cp <= 0xFFFF) {
            dst[o++] = (char16)cp;
        } else {
            if (o + 2 > maxLen - 1) break;  // don't split a surrogate pair
            cp -= 0x10000;
            dst[o++] = (char16)(0xD800 | (cp >> 10));
            dst[o++] = (char16)(0xDC00 | (cp & 0x3FF));
        }
    }
    dst[o] = 0;
}

// Lock the decode against the old byte-widen (which mojibake'd µ into
// 0xFFC2 0xFFB5). BMP two-byte, ASCII, and an astral surrogate pair.
namespace {
constexpr bool test_str_to_char16() {
    // `char16` is a signed 16-bit type, so compare code units as unsigned.
    char16 b[8] = {};
    str_to_char16(b, "\xC2\xB5", 8);  // U+00B5 MICRO SIGN
    if ((uint16_t)b[0] != 0x00B5 || b[1] != 0) return false;
    str_to_char16(b, "AB", 8);
    if ((uint16_t)b[0] != 0x41 || (uint16_t)b[1] != 0x42 || b[2] != 0) return false;
    str_to_char16(b, "\xF0\x9F\x98\x80", 8);  // U+1F600, surrogate pair
    if ((uint16_t)b[0] != 0xD83D || (uint16_t)b[1] != 0xDE00 || b[2] != 0) return false;
    return true;
}
static_assert(test_str_to_char16(), "str_to_char16 must decode UTF-8, not byte-widen");
}  // namespace

static void str_to_char8(char8* dst, const char* src, int maxLen) {
    int i = 0;
    for (; src[i] && i < maxLen - 1; i++) dst[i] = src[i];
    dst[i] = 0;
}

// UTF-16 -> UTF-8, the reverse of `str_to_char16`. Encodes the BMP
// (surrogate pairs are dropped - parameter entry text is effectively
// always in the BMP). `maxLen` bounds the output including the NUL.
static void char16_to_str(char* dst, const char16* src, int maxLen) {
    int o = 0;
    for (int i = 0; src[i] && o < maxLen - 1; i++) {
        unsigned int c = (unsigned int)(uint16_t)src[i];
        if (c < 0x80) {
            dst[o++] = (char)c;
        } else if (c < 0x800) {
            if (o + 2 > maxLen - 1) break;
            dst[o++] = (char)(0xC0 | (c >> 6));
            dst[o++] = (char)(0x80 | (c & 0x3F));
        } else {
            if (o + 3 > maxLen - 1) break;
            dst[o++] = (char)(0xE0 | (c >> 12));
            dst[o++] = (char)(0x80 | ((c >> 6) & 0x3F));
            dst[o++] = (char)(0x80 | (c & 0x3F));
        }
    }
    dst[o] = 0;
}

// ---------------------------------------------------------------------------
// FFI types (must match Rust ffi.rs)
// ---------------------------------------------------------------------------

struct Vst3PluginDescriptor {
    const char* name;
    const char* vendor;
    const char* url;
    const char* email;
    const char* version;
    uint8_t cid[16];
    const char* category;
    const char* subcategories;
    uint32_t num_inputs;
    uint32_t num_outputs;
    /* Per-bus structure of the first declared layout. The shim reports
     * one VST3 audio bus per entry (a sidechain input becomes its own
     * kBusType_Aux bus) instead of summing every bus into one. Bus count
     * and kind are consistent across a plugin's layouts; only widths
     * vary, so these describe every layout's structure. */
    uint32_t num_input_buses;
    uint32_t num_output_buses;
    /* One byte per bus: 0 = Main, 1 = Sidechain/Aux. Arrays of length
     * num_input_buses / num_output_buses. */
    const uint8_t* input_bus_kinds;
    const uint8_t* output_bus_kinds;
    /* Number of MIDI output ports (event output buses). Zero means the
     * host never allocates `ProcessData::outputEvents`, so the drain
     * loop after process() has nowhere to push and is a no-op. */
    int32_t midi_output_ports;
    /* Number of MIDI input ports (event input buses), decoupled from
     * `num_inputs` so an audio effect (with audio inputs) can still
     * take MIDI. Zero means no MIDI input. */
    int32_t midi_input_ports;
    /* Non-zero when the plugin processes natively in f64. Enables
     * kSample64 negotiation; blocks then route through process_f64. */
    int32_t supports_f64;
    /* Widest total output-channel count across ALL declared layouts (not
     * just the first). The per-channel output discard scratch is sized to
     * this so a host that negotiates a wider layout than the default gets a
     * distinct discard block per channel - sizing from num_outputs (the
     * first layout) would make two deactivated output channels of a wider
     * layout alias one block, which RawBufferScratch turns into two &mut
     * over the same memory (UB). */
    uint32_t max_outputs;
};

struct Vst3ParamDescriptor {
    uint32_t id;
    const char* name;
    const char* short_name;
    const char* units;
    double min;
    double max;
    double default_normalized;
    int32_t step_count;
    int32_t flags;
    const char* group;
};

struct Vst3NativeEvent {
    uint32_t kind;
    int32_t sample_offset;
    int32_t bus_index;
    uint32_t flags;
    int16_t channel;
    int16_t pitch;
    int32_t note_id;
    uint32_t type_id;
    int32_t length;
    float velocity;
    float tuning;
    double value;
    double ppq_position;
    const uint8_t* bytes;
    uint32_t len;
    uint8_t data1;
    uint8_t data2;
};
static_assert(sizeof(Vst3NativeEvent) == 72, "Vst3NativeEvent size");
static_assert(alignof(Vst3NativeEvent) == 8, "Vst3NativeEvent alignment");
static_assert(offsetof(Vst3NativeEvent, kind) == 0, "Vst3NativeEvent.kind");
static_assert(offsetof(Vst3NativeEvent, sample_offset) == 4, "Vst3NativeEvent.sample_offset");
static_assert(offsetof(Vst3NativeEvent, bus_index) == 8, "Vst3NativeEvent.bus_index");
static_assert(offsetof(Vst3NativeEvent, flags) == 12, "Vst3NativeEvent.flags");
static_assert(offsetof(Vst3NativeEvent, channel) == 16, "Vst3NativeEvent.channel");
static_assert(offsetof(Vst3NativeEvent, pitch) == 18, "Vst3NativeEvent.pitch");
static_assert(offsetof(Vst3NativeEvent, note_id) == 20, "Vst3NativeEvent.note_id");
static_assert(offsetof(Vst3NativeEvent, type_id) == 24, "Vst3NativeEvent.type_id");
static_assert(offsetof(Vst3NativeEvent, length) == 28, "Vst3NativeEvent.length");
static_assert(offsetof(Vst3NativeEvent, velocity) == 32, "Vst3NativeEvent.velocity");
static_assert(offsetof(Vst3NativeEvent, tuning) == 36, "Vst3NativeEvent.tuning");
static_assert(offsetof(Vst3NativeEvent, value) == 40, "Vst3NativeEvent.value");
static_assert(offsetof(Vst3NativeEvent, ppq_position) == 48, "Vst3NativeEvent.ppq_position");
static_assert(offsetof(Vst3NativeEvent, bytes) == 56, "Vst3NativeEvent.bytes");
static_assert(offsetof(Vst3NativeEvent, len) == 64, "Vst3NativeEvent.len");
static_assert(offsetof(Vst3NativeEvent, data1) == 68, "Vst3NativeEvent.data1");
static_assert(offsetof(Vst3NativeEvent, data2) == 69, "Vst3NativeEvent.data2");

enum Vst3SdkEventType : uint16_t {
    kVst3NoteOnEvent = 0,
    kVst3NoteOffEvent = 1,
    kVst3DataEvent = 2,
    kVst3PolyPressureEvent = 3,
    kVst3NoteExpressionValueEvent = 4,
    kVst3NoteExpressionTextEvent = 5,
    kVst3ChordEvent = 6,
    kVst3ScaleEvent = 7,
    kVst3LegacyMidiCcOutEvent = 65535,
};
static_assert(kVst3NoteOnEvent == 0 && kVst3NoteOffEvent == 1,
              "VST3 note event discriminants");
static_assert(kVst3DataEvent == 2 && kVst3PolyPressureEvent == 3,
              "VST3 data/pressure discriminants");
static_assert(kVst3NoteExpressionValueEvent == 4
                  && kVst3NoteExpressionTextEvent == 5,
              "VST3 expression discriminants");
static_assert(kVst3ChordEvent == 6 && kVst3ScaleEvent == 7,
              "VST3 chord/scale discriminants");
static_assert(kVst3LegacyMidiCcOutEvent == 65535,
              "VST3 legacy MIDI CC discriminant");

struct alignas(8) Vst3SdkEvent {
    int32 busIndex;
    int32 sampleOffset;
    double ppqPosition;
    uint16_t flags;
    uint16_t type;
    char pad[4];
    union {
        struct { int16_t channel; int16_t pitch; float tuning; float velocity; int32 length; int32 noteId; } noteOn;
        struct { int16_t channel; int16_t pitch; float velocity; int32 noteId; float tuning; } noteOff;
        struct { uint32_t size; uint32_t dataType; const uint8_t* bytes; } data;
        struct { int16_t channel; int16_t pitch; float pressure; int32 noteId; } polyPressure;
        struct { uint32_t typeId; int32_t noteId; double value; } noteExpressionValue;
        struct { uint8_t controlNumber; int8_t channel; int8_t value; int8_t value2; } midiCCOut;
    };
};
static_assert(sizeof(Vst3SdkEvent) == 48, "VST3 Event size");
static_assert(alignof(Vst3SdkEvent) == 8, "VST3 Event alignment");
static_assert(offsetof(Vst3SdkEvent, busIndex) == 0, "VST3 Event.busIndex");
static_assert(offsetof(Vst3SdkEvent, sampleOffset) == 4, "VST3 Event.sampleOffset");
static_assert(offsetof(Vst3SdkEvent, ppqPosition) == 8, "VST3 Event.ppqPosition");
static_assert(offsetof(Vst3SdkEvent, flags) == 16, "VST3 Event.flags");
static_assert(offsetof(Vst3SdkEvent, type) == 18, "VST3 Event.type");
static_assert(offsetof(Vst3SdkEvent, pad) == 20, "VST3 Event union padding");
static_assert(offsetof(Vst3SdkEvent, noteOn) == 24, "VST3 Event.noteOn");
static_assert(offsetof(Vst3SdkEvent, noteOn.channel) == 24, "VST3 NoteOn.channel");
static_assert(offsetof(Vst3SdkEvent, noteOn.pitch) == 26, "VST3 NoteOn.pitch");
static_assert(offsetof(Vst3SdkEvent, noteOn.tuning) == 28, "VST3 NoteOn.tuning");
static_assert(offsetof(Vst3SdkEvent, noteOn.velocity) == 32, "VST3 NoteOn.velocity");
static_assert(offsetof(Vst3SdkEvent, noteOn.length) == 36, "VST3 NoteOn.length");
static_assert(offsetof(Vst3SdkEvent, noteOn.noteId) == 40, "VST3 NoteOn.noteId");
static_assert(offsetof(Vst3SdkEvent, noteOff) == 24, "VST3 Event.noteOff");
static_assert(offsetof(Vst3SdkEvent, noteOff.channel) == 24, "VST3 NoteOff.channel");
static_assert(offsetof(Vst3SdkEvent, noteOff.pitch) == 26, "VST3 NoteOff.pitch");
static_assert(offsetof(Vst3SdkEvent, noteOff.velocity) == 28, "VST3 NoteOff.velocity");
static_assert(offsetof(Vst3SdkEvent, noteOff.noteId) == 32, "VST3 NoteOff.noteId");
static_assert(offsetof(Vst3SdkEvent, noteOff.tuning) == 36, "VST3 NoteOff.tuning");
static_assert(offsetof(Vst3SdkEvent, data) == 24, "VST3 Event.data");
static_assert(offsetof(Vst3SdkEvent, data.size) == 24, "VST3 Data.size");
static_assert(offsetof(Vst3SdkEvent, data.dataType) == 28, "VST3 Data.type");
static_assert(offsetof(Vst3SdkEvent, data.bytes) == 32, "VST3 Data.bytes");
static_assert(offsetof(Vst3SdkEvent, polyPressure) == 24, "VST3 Event.polyPressure");
static_assert(offsetof(Vst3SdkEvent, polyPressure.channel) == 24, "VST3 Pressure.channel");
static_assert(offsetof(Vst3SdkEvent, polyPressure.pitch) == 26, "VST3 Pressure.pitch");
static_assert(offsetof(Vst3SdkEvent, polyPressure.pressure) == 28, "VST3 Pressure.value");
static_assert(offsetof(Vst3SdkEvent, polyPressure.noteId) == 32, "VST3 Pressure.noteId");
static_assert(offsetof(Vst3SdkEvent, noteExpressionValue) == 24, "VST3 Event.noteExpression");
static_assert(offsetof(Vst3SdkEvent, noteExpressionValue.typeId) == 24, "VST3 Expression.typeId");
static_assert(offsetof(Vst3SdkEvent, noteExpressionValue.noteId) == 28, "VST3 Expression.noteId");
static_assert(offsetof(Vst3SdkEvent, noteExpressionValue.value) == 32, "VST3 Expression.value");
static_assert(offsetof(Vst3SdkEvent, midiCCOut) == 24, "VST3 Event.midiCCOut");
static_assert(offsetof(Vst3SdkEvent, midiCCOut.controlNumber) == 24, "VST3 MIDI CC.number");
static_assert(offsetof(Vst3SdkEvent, midiCCOut.channel) == 25, "VST3 MIDI CC.channel");
static_assert(offsetof(Vst3SdkEvent, midiCCOut.value) == 26, "VST3 MIDI CC.value");
static_assert(offsetof(Vst3SdkEvent, midiCCOut.value2) == 27, "VST3 MIDI CC.value2");

struct Vst3Transport {
    int32_t playing;
    int32_t recording;
    double tempo;
    int32_t time_sig_num;
    int32_t time_sig_den;
    double position_samples;
    double position_beats;
    double bar_start_beats;
    double cycle_start_beats;
    double cycle_end_beats;
    int32_t cycle_active;
};

struct Vst3ParamChange {
    uint32_t id;
    int32_t sample_offset;
    double value; // plain value
};

struct Vst3Callbacks {
    void* (*create)();
    void (*destroy)(void*);
    void (*reset)(void*, double, uint32_t, int32_t);
    uint32_t (*process)(void*, const float**, float**, uint32_t, uint32_t,
                        const uint32_t*, uint32_t, uint32_t,
                        const uint32_t*, uint32_t, uint32_t, uint32_t,
                        const Vst3Transport*, const Vst3ParamChange*, uint32_t, int32_t);
    /* 64-bit twin of process. Exactly one of the two runs per block,
     * chosen by the sample size negotiated in setupProcessing. */
    uint32_t (*process_f64)(void*, const double**, double**, uint32_t, uint32_t,
                            const uint32_t*, uint32_t, uint32_t,
                            const uint32_t*, uint32_t, uint32_t, uint32_t,
                            const Vst3Transport*, const Vst3ParamChange*, uint32_t, int32_t);
    uint32_t (*param_count)(void*);
    double (*param_get_value)(void*, uint32_t);
    void (*param_set_value)(void*, uint32_t, double);
    double (*param_normalize)(void*, uint32_t, double);
    double (*param_denormalize)(void*, uint32_t, double);
    uint32_t (*param_format)(void*, uint32_t, double, char*, uint32_t);
    int32_t (*param_parse)(void*, uint32_t, const char*, double*);
    void (*state_save)(void*, uint8_t**, uint32_t*);
    /* Returns 1 when the blob was accepted (moose envelope, or the
     * plugin's migrate_state translated it), 0 when the load failed -
     * setState forwards that to the host as kResultFalse. */
    int32_t (*state_load)(void*, const uint8_t*, uint32_t);
    void (*state_free)(uint8_t*, uint32_t);
    // Latency + tail
    uint32_t (*get_latency)(void*);
    uint32_t (*get_tail)(void*);
    void (*begin_input_events)(void*, uint32_t /*num_frames*/);
    uint32_t (*push_input_event)(void*, const Vst3NativeEvent*);
    void (*begin_output_events)(void*);
    uint32_t (*next_output_event)(void*, Vst3NativeEvent*);
    void (*commit_output_event)(void*);
    void (*finish_output_events)(void*, uint32_t);
    // GUI
    int32_t (*gui_has_editor)(void*);
    void (*gui_get_size)(void*, uint32_t*, uint32_t*);
    void (*gui_open)(void*, void*);
    void (*gui_close)(void*);
    void (*gui_set_content_scale)(void*, double);
    int32_t (*gui_can_resize)(void*);
    void (*gui_check_size_constraint)(void*, uint32_t*, uint32_t*);
    void (*gui_set_size)(void*, uint32_t, uint32_t);
    // IMidiMapping: resolve a (bus, channel, controllerNumber) query to
    // a parameter id. Returns 1 (kResultOk) + writes *out on a hit.
    int32_t (*midi_mapping_get_param_id)(void*, int32_t, int16_t, int16_t, uint32_t*);
    // Process-emitted parameter output - shim queries after `process`
    // and adds each to the host's `outputParameterChanges` queue.
    // `value` is normalized [0,1]; `sample_offset` is block-relative.
    uint32_t (*get_output_param_count)(void*);
    void (*get_output_param)(void*, uint32_t /*index*/, uint32_t* /*id*/,
                             int32_t* /*sample_offset*/, double* /*value*/);
    // IComponent::setActive. `active != 0` between activate and
    // deactivate; lets Rust apply a suspended-plugin state load
    // synchronously instead of stranding it in the audio-thread queue.
    void (*set_active)(void*, int32_t /*active*/);
    // Return the `bus_layouts()` index whose main-bus channel counts are
    // (in_ch, out_ch), or -1 if none. Static per plugin type (no ctx);
    // `setBusArrangements` uses it to accept alternate layouts.
    int32_t (*match_bus_layout)(uint32_t /*in_ch*/, uint32_t /*out_ch*/);
    // Per-bus channel width of a declared layout, for per-bus getBusInfo /
    // getBusArrangement. `is_output != 0` selects the output direction.
    uint32_t (*layout_bus_channels)(uint32_t /*layout_index*/, int32_t /*is_output*/,
                                    uint32_t /*bus_index*/);
    // Match a host per-bus arrangement (arrays of per-bus channel counts)
    // to a declared layout index, or -1. Replaces match_bus_layout for
    // setBusArrangements so a sidechain bus is matched on its own width.
    int32_t (*match_bus_layout_perbus)(const uint32_t* /*in_channels*/, uint32_t /*num_in*/,
                                       const uint32_t* /*out_channels*/, uint32_t /*num_out*/);
    // Non-zero when a param is CHUNKED (sample-accurate automation). The
    // shim skips its block-rate pre-commit for these on a processing
    // block, so process_chunked applies each point at its sample offset
    // instead of the end value landing from sample 0.
    int32_t (*param_is_chunked)(void*, uint32_t);
    // Runtime title for a param: -1 = none (keep the static info),
    // otherwise 0 / 1 = hidden. Writes the NUL-terminated title.
    int32_t (*param_presentation)(void*, uint32_t, char*, uint32_t);
    // Bumped whenever any param_presentation result changes.
    uint64_t (*param_presentation_revision)(void*);
    // Output audio bus title (the plugin's runtime name, else the layout's):
    // 1 when written, 0 for none (keep the generic title).
    int32_t (*output_bus_name)(void*, uint32_t /*layout*/, uint32_t /*index*/, char*, uint32_t);
    // Bumped whenever any output_bus_name result changes.
    uint64_t (*output_bus_names_revision)(void*);
};

// ---------------------------------------------------------------------------
// Global registration (set by Rust)
// ---------------------------------------------------------------------------

static const Vst3PluginDescriptor* g_desc = nullptr;
static const Vst3Callbacks* g_cb = nullptr;
static const Vst3ParamDescriptor* g_params = nullptr;
static uint32_t g_num_params = 0;

// Unit info (parameter groups) - built at registration time
static const int kMaxUnits = 64;
struct UnitEntry { int32 id; int32 parentId; char name[128]; };
static UnitEntry g_units[kMaxUnits];
static int g_num_units = 0;
// Maps param index → unit ID
static int32 g_param_unit_id[4096];

static void build_unit_map() {
    // Unit 0 = root (always exists)
    g_num_units = 1;
    g_units[0].id = 0;
    g_units[0].parentId = -1; // kNoParentUnitId
    strncpy(g_units[0].name, "Root", sizeof(g_units[0].name));

    for (uint32_t i = 0; i < g_num_params && i < 4096; i++) {
        const char* group = g_params[i].group;
        if (!group || group[0] == 0) {
            g_param_unit_id[i] = 0; // root unit
            continue;
        }
        // Find or create unit for this group
        int32 unitId = -1;
        for (int u = 1; u < g_num_units; u++) {
            if (strcmp(g_units[u].name, group) == 0) {
                unitId = g_units[u].id;
                break;
            }
        }
        if (unitId < 0 && g_num_units < kMaxUnits) {
            unitId = g_num_units;
            g_units[g_num_units].id = unitId;
            g_units[g_num_units].parentId = 0; // parent = root
            strncpy(g_units[g_num_units].name, group, sizeof(g_units[0].name));
            g_num_units++;
        }
        g_param_unit_id[i] = (unitId >= 0) ? unitId : 0;
    }
}

// ---------------------------------------------------------------------------
// VST3 structs (matching SDK layout)
// ---------------------------------------------------------------------------

struct ProcessSetup {
    int32 processMode;
    int32 symbolicSampleSize;
    int32 maxSamplesPerBlock;
    double sampleRate;
};

struct AudioBusBuffers {
    int32 numChannels;
    uint64_t silenceFlags;
    union { float** channelBuffers32; double** channelBuffers64; };
};

// VST3 ProcessContext (transport info from host)
struct Vst3ProcessContext {
    uint32 state;                 // combination of StatesAndFlags
    double sampleRate;
    // These three are `TSamples` (int64) in the VST3 SDK, NOT double.
    // Same width, so the later doubles stay aligned - but reading
    // projectTimeSamples as a double reinterprets the integer's bits
    // (a small sample count decodes to a ~0 denormal).
    int64_t projectTimeSamples;   // project time in samples
    int64_t systemTime;           // system time in nanoseconds
    int64_t continousTimeSamples;
    double projectTimeMusic;      // musical position in quarter notes
    double barPositionMusic;      // last bar start position in quarter notes
    double cycleStartMusic;       // cycle start in quarter notes
    double cycleEndMusic;         // cycle end in quarter notes
    double tempo;                 // tempo in BPM
    int32 timeSigNumerator;
    int32 timeSigDenominator;
    // chord + key signature fields follow but we don't need them
};

// ProcessContext state flags. Values from the VST3 SDK
// (ivstprocesscontext.h StatesAndFlags) - kBarPositionValid and
// kTimeSigValid are NOT adjacent, and kCycleActive (loop engaged) is
// distinct from kCycleValid (loop markers hold valid positions).
enum {
    kPlaying          = 1 << 1,
    kCycleActive      = 1 << 2,
    kRecording        = 1 << 3,
    kProjectTimeMusicValid = 1 << 9,
    kTempoValid       = 1 << 10,
    kBarPositionValid = 1 << 11,
    kCycleValid       = 1 << 12,
    kTimeSigValid     = 1 << 13,
};

struct ProcessData {
    int32 processMode;
    int32 symbolicSampleSize;
    int32 numSamples;
    int32 numInputs;
    int32 numOutputs;
    AudioBusBuffers* inputs;
    AudioBusBuffers* outputs;
    void* inputParameterChanges;   // IParameterChanges*
    void* outputParameterChanges;
    void* inputEvents;             // IEventList*
    void* outputEvents;
    Vst3ProcessContext* processContext;
};

struct BusInfo {
    int32 mediaType;
    int32 direction;
    int32 channelCount;
    char16 name[128];
    int32 busType;
    uint32 flags;
};

struct RoutingInfo { int32 mediaType; int32 busIndex; int32 channel; };

struct ParameterInfo {
    uint32 id;
    char16 title[128];
    char16 shortTitle[128];
    char16 units[128];
    int32 stepCount;
    double defaultNormalizedValue;
    int32 unitId;
    int32 flags;
};

struct PFactoryInfo {
    char8 vendor[64];
    char8 url[256];
    char8 email[128];
    int32 flags;
};

struct PClassInfo {
    TUID cid;
    int32 cardinality;
    char8 category[32];
    char8 name[64];
};

// ---------------------------------------------------------------------------
// Component: IComponent + IAudioProcessor + IEditController
// ---------------------------------------------------------------------------

struct MooseComponentCOM; // forward declaration
class MooseComponent;     // forward declaration for ctx mapping
struct MoosePlugView;     // forward decl - full def lives below in the
                          // GUI section; MooseComponent only needs a
                          // pointer to it for the `plugView` field.

// ---------------------------------------------------------------------------
// Bounded host state streams
// ---------------------------------------------------------------------------

// IBStream vtable: queryInterface, addRef, release, read, write, seek, tell.
struct IBStreamVtbl {
    tresult (*queryInterface)(void*, const TUID, void**);
    uint32  (*addRef)(void*);
    uint32  (*release)(void*);
    tresult (*read)(void*, void*, int32, int32*);
    tresult (*write)(void*, void*, int32, int32*);
    tresult (*seek)(void*, int64_t, int32, int64_t*);
    tresult (*tell)(void*, int64_t*);
};

// Largest state blob accepted from, or handed to, a host stream.
static constexpr int32 kMaxHostStateBytes = 32 * 1024 * 1024;

extern "C" {

// Read one bounded state blob from a host IBStream. Returns a malloc'd
// buffer (caller frees) and its length, or nullptr when the stream is
// empty, errors before any data, reports a byte count outside the
// request, or runs past kMaxHostStateBytes. A short final read is
// valid. Exported so the Rust tests can drive it with a fake stream.
uint8_t* moose_vst3_read_state_stream(void* stream, int32* outLen) {
    *outLen = 0;
    if (!stream) return nullptr;
    auto* vtbl = *reinterpret_cast<IBStreamVtbl**>(stream);
    uint8_t buf[4096];
    uint8_t* data = nullptr;
    int32 total = 0;
    int32 capacity = 0;
    for (;;) {
        int32 bytesRead = 0;
        tresult r = vtbl->read(stream, buf, (int32)sizeof(buf), &bytesRead);
        if (bytesRead < 0 || bytesRead > (int32)sizeof(buf)) { free(data); return nullptr; }
        if (bytesRead == 0) break;
        if (total > kMaxHostStateBytes - bytesRead) { free(data); return nullptr; }
        const int32 nextTotal = total + bytesRead;
        if (nextTotal > capacity) {
            // Geometric growth: one realloc per doubling, not per chunk.
            int32 nextCapacity = capacity == 0 ? (int32)sizeof(buf) : capacity;
            while (nextCapacity < nextTotal) {
                nextCapacity = nextCapacity > kMaxHostStateBytes / 2
                    ? kMaxHostStateBytes : nextCapacity * 2;
            }
            auto* grown = (uint8_t*)realloc(data, (size_t)nextCapacity);
            if (!grown) { free(data); return nullptr; }
            data = grown;
            capacity = nextCapacity;
        }
        memcpy(data + total, buf, (size_t)bytesRead);
        total = nextTotal;
        if (r != kResultOk) break;
    }
    if (total == 0) { free(data); return nullptr; }
    *outLen = total;
    return data;
}

// Write a whole state blob to a host IBStream, looping over partial
// writes. Returns 1 when every byte landed, 0 when the blob is empty or
// oversized, or the stream reports no progress, a count outside the
// request, or an error before the blob is complete.
int32_t moose_vst3_write_state_stream(void* stream, const uint8_t* data, uint32_t len) {
    if (!stream || !data || len == 0 || len > (uint32_t)kMaxHostStateBytes) return 0;
    auto* vtbl = *reinterpret_cast<IBStreamVtbl**>(stream);
    uint32_t offset = 0;
    while (offset < len) {
        const int32 request = (int32)(len - offset);
        int32 written = 0;
        tresult r = vtbl->write(stream, (void*)(data + offset), request, &written);
        if (written <= 0 || written > request) return 0;
        offset += (uint32_t)written;
        if (r != kResultOk && offset < len) return 0;
    }
    return 1;
}

} // extern "C"

// Global ctx→component mapping for extern "C" host-notification callbacks
static constexpr int kMaxInstances = 64;
static void* g_ctx_map_key[kMaxInstances] = {};
static MooseComponent* g_ctx_map_comp[kMaxInstances] = {};

#if __APPLE__
// Resolves the shim's opaque ctx key to its live component, or null once
// the component's destructor has unregistered it. Both defined below,
// once MooseComponent is complete; declared here so the constructor can
// wire the dispatch source's event handler.
static MooseComponent* ctx_lookup(void* ctx);
static void moose_vst3_restart_source_event(void* key);
#endif

#if defined(__linux__)
// Steinberg::Linux::ITimerHandler embedded in each component so
// `&component.restartTimer` is a valid ITimerHandler* to hand the host's
// run loop. It is not independently reference counted: the component owns
// it and unregisters it from the run loop before its own teardown, so
// addRef / release are inert. `onTimer` (defined below, once
// MooseComponent is complete) drains the pending restart on the host UI
// thread - the run loop calls it there. A timer, not an fd handler, so
// nothing here or in the audio-thread path makes a syscall the host's
// sandbox could reject; the flush just polls the pending-restart bit.
struct ITimerHandlerVtbl {
    tresult (*queryInterface)(void*, const TUID, void**);
    uint32  (*addRef)(void*);
    uint32  (*release)(void*);
    void    (*onTimer)(void*);
};
struct RestartTimerHandler {
    ITimerHandlerVtbl* vtbl;
    MooseComponent* comp;
};
// Bodies live below MooseComponent (they call flushPendingRestart); the
// vtable can take their addresses here since they're forward-declared.
static tresult th_queryInterface(void*, const TUID, void**);
static uint32  th_addRef(void*);
static uint32  th_release(void*);
static void    th_onTimer(void*);
static ITimerHandlerVtbl g_restart_timer_handler_vtbl = {
    th_queryInterface, th_addRef, th_release, th_onTimer,
};
// IRunLoop vtable (FUnknown + 4): [5] registerTimer(handler, ms),
// [6] unregisterTimer(handler). Named constants keep the vtable
// arithmetic self-documenting, matching kPlugFrameResizeViewIndex.
static constexpr int32_t kRunLoopRegisterTimerIndex = 5;
static constexpr int32_t kRunLoopUnregisterTimerIndex = 6;
#endif

#if defined(__linux__) || defined(_WIN32)
// Poll interval for the restart timer (Linux run-loop timer, Windows
// thread timer). A latency re-report only needs to reach the host
// "soon"; 30 ms is imperceptible and keeps the idle cost negligible.
static constexpr uint64_t kRestartTimerIntervalMs = 30;
#endif

#if defined(_WIN32)
// TIMERPROC for the per-component restart timer (declared here so the
// constructor can pass it to SetTimer; body below, once MooseComponent
// is complete). The message pump of the thread that created the timer -
// the host UI thread - calls it, so the flush lands where VST3 wants
// restartComponent. It resolves the component through the live-instance
// map by timer id, so a WM_TIMER dispatched after teardown is a no-op.
static void CALLBACK moose_vst3_restart_timer_proc(HWND, UINT, UINT_PTR, DWORD);
#endif

// Map a channel count to a canonical VST3 SpeakerArrangement. Hosts and
// validators compare the exact bitmask (pluginval's arrangement
// consistency check, JUCE's channel-set mapping), so the low-bits
// popcount trick `(1<<ch)-1` mislabels channels: bit 0 is Left, not Mono,
// and 4 channels reads as 3.1 (L R C Lfe) rather than quad. Named
// arrangements for the common counts; the popcount fallback keeps the
// right channelCount for exotic widths, where that's all a host keys on.
static constexpr uint64_t speaker_arr_for_channels(uint32_t ch) {
    // VST3 SpeakerArr speaker bits (vstspeaker.h).
    const uint64_t L = 1ull << 0, R = 1ull << 1, C = 1ull << 2, Lfe = 1ull << 3,
                   Ls = 1ull << 4, Rs = 1ull << 5, Sl = 1ull << 9, Sr = 1ull << 10,
                   M = 1ull << 19;
    switch (ch) {
        case 1: return M;                          // kMono (Mono center, not Left)
        case 2: return L | R;                      // kStereo
        case 4: return L | R | Ls | Rs;            // k40Music (quadraphonic)
        case 6: return L | R | C | Lfe | Ls | Rs;  // k51
        case 8: return L | R | C | Lfe | Ls | Rs | Sl | Sr;  // k71Music
        default: return ((uint64_t)1 << ch) - 1;   // popcount fallback
    }
}

// Lock the canonical arrangements against the popcount trick that
// mislabels them. Each named width's popcount also equals the channel
// count getBusInfo reports, so the two stay consistent.
static_assert(speaker_arr_for_channels(1) == (1ull << 19), "mono must be kMono, not kSpeakerL");
static_assert(speaker_arr_for_channels(2) == 0x3ull, "stereo = L|R");
static_assert(speaker_arr_for_channels(4) == 0x33ull, "quad = L|R|Ls|Rs, not 3.1");
static_assert(speaker_arr_for_channels(6) == 0x3Full, "5.1 = L R C Lfe Ls Rs");
static_assert(speaker_arr_for_channels(8) == 0x63Full, "7.1 = 5.1 + Sl + Sr");

// One activation bit per declared bus. Every valid VST3 audio bus consumes at
// least one channel, and registration rejects layouts wider than
// kMaxProcChannels, so the channel cap is also a safe bus-count cap. Keeping
// this as an always-lock-free scalar makes activateBus/process handoff
// allocation- and lock-free.
static_assert(std::atomic<uint32_t>::is_always_lock_free,
              "VST3 audio-bus activation requires lock-free uint32 atomics");

static uint32_t default_active_bus_mask(uint32_t count, const uint8_t* kinds) {
    uint32_t mask = 0;
    const uint32_t bounded = count < kMaxProcChannels ? count : kMaxProcChannels;
    for (uint32_t i = 0; i < bounded; i++) {
        // Main buses are active by default. Sidechain/aux buses are optional:
        // the host explicitly activates whichever ones it routes.
        if (!kinds || kinds[i] == 0)
            mask |= uint32_t{1} << i;
    }
    return mask;
}

class MooseComponent {
    std::atomic<int32> refCount{1};
    void* ctx;
    double sampleRate;
    uint32_t maxFrames;
    /* Sample size negotiated in setupProcessing; process() rejects a
     * block whose symbolicSampleSize disagrees. */
    int32 procSampleSize = kSample32;
    /* ProcessModes from setupProcessing (kRealtime / kPrefetch /
     * kOffline). Replayed into reset() at setActive; process() reads
     * the per-block ProcessData::processMode directly. */
    int32 procMode = 0;
    /* Currently-selected main-bus channel counts for THIS instance.
     * Seeded to the default (first) layout; setBusArrangements moves them
     * to any other layout the plugin declared in bus_layouts(), and
     * getBusInfo / getBusArrangement report them so the host sees this
     * instance's active layout. Per-instance, not a file-scope global, so
     * two instances of one plugin negotiated to different widths (a 5.1
     * track and a stereo track) don't clobber each other's arrangement. */
    uint32_t cur_in = g_desc ? g_desc->num_inputs : 0;
    uint32_t cur_out = g_desc ? g_desc->num_outputs : 0;
    // Index into bus_layouts() this instance currently runs in.
    // setBusArrangements moves it; getBusInfo / getBusArrangement read
    // per-bus widths from it via g_cb->layout_bus_channels.
    uint32_t cur_layout = 0;
    // Topology stays fixed while activateBus changes which declared buses
    // participate. Main buses start active; aux buses start inactive.
    std::atomic<uint32_t> inputBusActive{0};
    std::atomic<uint32_t> outputBusActive{0};
    // Negotiated per-bus widths cached outside process(). Calling the Rust
    // layout callback from the audio thread would reconstruct bus_layouts()
    // and may allocate; these fixed arrays make render-time lookup O(1).
    uint32_t inputBusChannels[kMaxProcChannels] = {};
    uint32_t outputBusChannels[kMaxProcChannels] = {};
    /* Omitted-bus scratch (see process()): the host may drop the
     * AudioBusBuffers for a deactivated last bus entirely, but the
     * plug-in negotiated fixed widths - a missing input side reads
     * from one shared zeroed block, a missing output side writes to
     * per-channel discard blocks. Allocated in setupProcessing (off
     * the audio thread), sized maxFrames * sizeof(double) per channel
     * so one allocation covers both sample widths. */
    uint8_t* silenceScratch = nullptr;
    uint8_t* trashScratch = nullptr;

    void refreshBusChannels() {
        memset(inputBusChannels, 0, sizeof(inputBusChannels));
        memset(outputBusChannels, 0, sizeof(outputBusChannels));
        if (!g_desc || !g_cb || !g_cb->layout_bus_channels) return;
        const uint32_t inputs = g_desc->num_input_buses < kMaxProcChannels
            ? g_desc->num_input_buses
            : kMaxProcChannels;
        const uint32_t outputs = g_desc->num_output_buses < kMaxProcChannels
            ? g_desc->num_output_buses
            : kMaxProcChannels;
        for (uint32_t b = 0; b < inputs; b++)
            inputBusChannels[b] = g_cb->layout_bus_channels(cur_layout, 0, b);
        for (uint32_t b = 0; b < outputs; b++)
            outputBusChannels[b] = g_cb->layout_bus_channels(cur_layout, 1, b);
    }
public:
    // cb_create (Rust `_create`) returns null when the author's create() /
    // init() panicked; a component with a null ctx is an inert zombie that
    // guards every callback but does nothing. The factory checks this to
    // fail createInstance rather than hand the host a do-nothing plugin.
    bool constructed() const { return ctx != nullptr; }
    void* rustContext() const { return ctx; }

    void* componentHandler;  // IComponentHandler*, stored with addRef
    // Pending IComponentHandler::restartComponent flags (RestartFlags
    // bitmask, e.g. kLatencyChanged = 8). The audio thread sets bits via
    // moose_vst3_mark_restart; flushPendingRestart() drains them on a
    // host main-thread callback (getParamNormalized / perform / endEdit),
    // so restartComponent - which VST3 wants off the audio thread and on
    // the UI thread - never fires from the render or a moose-owned thread.
    std::atomic<int32_t> pendingRestart{0};
#if __APPLE__
    // Main-queue dispatch source. The audio thread signals it (lock-free,
    // alloc-free) on a latency change; its handler drains pendingRestart
    // on the host main thread - the editor-closed / no-param-poll path
    // the callback hooks below can't cover. Null if creation failed.
    dispatch_source_t restartSource = nullptr;
#endif
#if defined(__linux__)
    // Linux run-loop bridge. `restartTimer` is the ITimerHandler we
    // register with the host's `runLoop` (queried off the plug frame in
    // `updateRunLoopRegistration`); while an editor is attached the run
    // loop calls onTimer on its UI thread, which flushes pendingRestart
    // there. `runLoop` is the host IRunLoop*, held with an addRef only
    // while the timer is registered; null when no editor is attached
    // (fall back to the param-poll flush).
    RestartTimerHandler restartTimer{&g_restart_timer_handler_vtbl, nullptr};
    void* runLoop = nullptr;
#endif
#if defined(_WIN32)
    // Null-HWND thread timer id from SetTimer. Its TIMERPROC runs on the
    // instantiating (host UI) thread's message pump and drains
    // pendingRestart there - the editor-closed / no-param-poll path the
    // callback hooks below can't cover. 0 if creation failed (fall back
    // to the param-poll / edit-gesture flush).
    UINT_PTR restartTimerId = 0;
#endif
    bool inPerformEdit;       // feedback guard: skip setParamNormalized during performEdit
    // Live plug view created by `createView()`. Set there, nulled in
    // `pv_release` when refcount -> 0. `moose_vst3_request_resize`
    // dereferences via this slot rather than holding a `MoosePlugView*`
    // in a Rust closure - hosts that release and re-create the view
    // (Cubase theme change, Live dock/undock) would otherwise leave
    // a dangling pointer in the closure.
    MoosePlugView* plugView;

    MooseComponent() : ctx(nullptr), sampleRate(44100), maxFrames(1024),
                       componentHandler(nullptr), inPerformEdit(false),
                       plugView(nullptr) {
        if (g_desc) {
            inputBusActive.store(
                default_active_bus_mask(g_desc->num_input_buses, g_desc->input_bus_kinds),
                std::memory_order_relaxed);
            outputBusActive.store(
                default_active_bus_mask(g_desc->num_output_buses, g_desc->output_bus_kinds),
                std::memory_order_relaxed);
        }
        refreshBusChannels();
        if (g_cb) {
            ctx = g_cb->create();
            if (ctx) {
                for (int i = 0; i < kMaxInstances; i++) {
                    if (!g_ctx_map_key[i]) {
                        g_ctx_map_key[i] = ctx;
                        g_ctx_map_comp[i] = this;
                        break;
                    }
                }
#if __APPLE__
                // Instances are created on the main thread, so setting up
                // a main-queue source here is safe. Context is the stable
                // ctx key, so the handler resolves the component safely
                // even if it is mid-teardown.
                restartSource = dispatch_source_create(
                    DISPATCH_SOURCE_TYPE_DATA_OR, 0, 0, dispatch_get_main_queue());
                if (restartSource) {
                    dispatch_set_context(restartSource, ctx);
                    dispatch_source_set_event_handler_f(
                        restartSource, moose_vst3_restart_source_event);
                    dispatch_resume(restartSource);
                }
#endif
#if defined(__linux__)
                // No syscall here - just wire the timer handler's
                // back-pointer. Registration with the host run loop waits
                // until a plug frame arrives (`updateRunLoopRegistration`),
                // so instantiation in the host's sandboxed scan/host
                // process makes no run-loop or kernel calls at all.
                restartTimer.comp = this;
#endif
#if defined(_WIN32)
                // Instances are created on the host UI thread, so the
                // thread timer binds to its message queue and WM_TIMER
                // fires from the host's pump. A pump-less scan thread
                // just never dispatches it - harmless, the param-poll
                // flush still covers that instance.
                restartTimerId = SetTimer(
                    nullptr, 0, (UINT)kRestartTimerIntervalMs,
                    moose_vst3_restart_timer_proc);
#endif
            }
        }
    }

    ~MooseComponent() {
        if (componentHandler) {
            auto release_fn = (uint32 (*)(void*))(*(void***)componentHandler)[2];
            release_fn(componentHandler);
        }
        if (ctx) {
            for (int i = 0; i < kMaxInstances; i++) {
                if (g_ctx_map_key[i] == ctx) {
                    g_ctx_map_key[i] = nullptr;
                    g_ctx_map_comp[i] = nullptr;
                    break;
                }
            }
        }
#if __APPLE__
        // Unregister above happens first, so any in-flight handler
        // resolves null and never touches this freed instance. cancel()
        // stops further invocations; release() is safe mid-handler
        // (libdispatch retains the source across a running handler).
        if (restartSource) {
            dispatch_source_cancel(restartSource);
            dispatch_release(restartSource);
            restartSource = nullptr;
        }
#endif
#if defined(__linux__)
        // Unregister the timer (releases the held IRunLoop) so the host
        // never calls onTimer on a freed component. A well-behaved host
        // has already dropped the frame via setFrame(nullptr) / view
        // release, making this a no-op; the call covers hosts that
        // release the component without doing so.
        updateRunLoopRegistration(nullptr);
#endif
#if defined(_WIN32)
        // Map unregistration above happens first, so a WM_TIMER already
        // in the queue resolves no component and is a safe no-op. Hosts
        // release on the creating (UI) thread, where KillTimer removes
        // the thread timer; from any other thread it fails and the
        // orphaned proc keeps no-oping via the map - never this instance.
        if (restartTimerId) {
            KillTimer(nullptr, restartTimerId);
            restartTimerId = 0;
        }
#endif
        free(silenceScratch);
        free(trashScratch);
        if (g_cb && ctx) g_cb->destroy(ctx);
    }

    // --- FUnknown ---
    // Note: queryInterface returns pointers into the containing MooseComponentCOM,
    // NOT into this MooseComponent. The caller (vtable functions) must pass the
    // correct COM wrapper pointer.

    // Defined after MooseComponentCOM (needs complete type)
    tresult queryInterface(void* comBase, const TUID iid, void** obj);

    uint32 addRef() { return ++refCount; }
    uint32 release() { return --refCount; }

    // --- IPluginBase ---

    tresult initialize(void* /*context*/) {
        if (g_cb && ctx) {
            // initialize is a no-op for now
        }
        return kResultOk;
    }

    tresult terminate() { return kResultOk; }

    // --- IComponent ---

    tresult getControllerClassId(TUID /*classId*/) { return kNotImplemented; }
    tresult setIoMode(int32) { return kResultOk; }

    int32 getBusCount(int32 type, int32 dir) {
        if (!g_desc) return 0;
        if (type == kAudio) {
            // Advertise an audio bus only when that side actually has
            // channels. A pure-MIDI note effect (num_inputs ==
            // num_outputs == 0) carries event buses only; a phantom
            // audio output bus with channelCount 0 is an invalid VST3
            // arrangement that strict hosts reject - Bitwig throws an
            // NPE reading the empty channel array during device load and
            // crashes. Symmetric with the input side, which already
            // guards on num_inputs.
            return (dir == kInput) ? (int32)g_desc->num_input_buses
                                   : (int32)g_desc->num_output_buses;
        }
        if (type == kEvent && dir == kInput) {
            return g_desc->midi_input_ports;
        }
        if (type == kEvent && dir == kOutput) {
            return g_desc->midi_output_ports;
        }
        return 0;
    }

    tresult getBusInfo(int32 type, int32 dir, int32 index, BusInfo* bus) {
        if (!bus || !g_desc) return kInvalidArgument;
        // For an event bus, `channelCount` is the number of MIDI channels
        // the bus carries. moose delivers/emits all 16 (the `channel`
        // nibble round-trips end-to-end), so advertise 16 - `1` would let
        // a channel-aware host route only channel 1.
        if (type == kEvent && dir == kInput && index >= 0 && index < g_desc->midi_input_ports) {
            bus->mediaType = kEvent; bus->direction = kInput; bus->channelCount = 16;
            // Number the ports only when there's more than one so the
            // single-port case keeps its plain "Event In" label.
            char name[32];
            if (g_desc->midi_input_ports > 1) snprintf(name, sizeof(name), "Event In %d", index + 1);
            else snprintf(name, sizeof(name), "Event In");
            str_to_char16(bus->name, name, 128);
            bus->busType = kMain; bus->flags = 1; // kDefaultActive
            return kResultOk;
        }
        if (type == kEvent && dir == kOutput && index >= 0 && index < g_desc->midi_output_ports) {
            bus->mediaType = kEvent; bus->direction = kOutput; bus->channelCount = 16;
            char name[32];
            if (g_desc->midi_output_ports > 1) snprintf(name, sizeof(name), "Event Out %d", index + 1);
            else snprintf(name, sizeof(name), "Event Out");
            str_to_char16(bus->name, name, 128);
            bus->busType = kMain; bus->flags = 1; // kDefaultActive
            return kResultOk;
        }
        if (type != kAudio) return kInvalidArgument;
        // One entry per declared bus. A sidechain/aux input (kind byte 1)
        // reports kBusType_Aux so the host offers it as a separate
        // routable input; the main bus stays kBusType_Main.
        const uint32_t busCount = (dir == kInput) ? g_desc->num_input_buses
                                                  : g_desc->num_output_buses;
        const uint8_t* kinds = (dir == kInput) ? g_desc->input_bus_kinds
                                               : g_desc->output_bus_kinds;
        if ((dir == kInput || dir == kOutput) && index >= 0 &&
                (uint32_t)index < busCount) {
            bus->mediaType = kAudio; bus->direction = dir;
            bus->channelCount = (g_cb && g_cb->layout_bus_channels)
                ? (int32)g_cb->layout_bus_channels(cur_layout, dir == kOutput, (uint32_t)index)
                : 0;
            const bool aux = kinds && kinds[index] != 0;
            const char* nm = (dir == kInput) ? (aux ? "Sidechain" : "Input")
                                             : (aux ? "Aux Output" : "Output");
            char named[128];
            if (dir == kOutput && g_cb && g_cb->output_bus_name &&
                    g_cb->output_bus_name(ctx, cur_layout, (uint32_t)index, named, sizeof(named)) == 1) {
                nm = named;
            }
            str_to_char16(bus->name, nm, 128);
            bus->busType = aux ? kAux : kMain;
            // kDefaultActive is truthful per role: the main path is live on a
            // fresh instance, while aux buses wait for activateBus.
            bus->flags = aux ? 0 : 1;
            return kResultOk;
        }
        return kInvalidArgument;
    }

    tresult getRoutingInfo(RoutingInfo*, RoutingInfo*) { return kNotImplemented; }
    tresult activateBus(int32 type, int32 dir, int32 index, TBool state) {
        if (!g_desc || index < 0) return kInvalidArgument;

        // Event buses have no audio-buffer state to track, but accept valid
        // host lifecycle calls as before.
        if (type == kEvent) {
            const int32 count = dir == kInput ? g_desc->midi_input_ports
                                : dir == kOutput ? g_desc->midi_output_ports
                                                 : 0;
            return index < count ? kResultOk : kInvalidArgument;
        }
        if (type != kAudio || (dir != kInput && dir != kOutput))
            return kInvalidArgument;

        const uint32_t count = dir == kInput ? g_desc->num_input_buses
                                              : g_desc->num_output_buses;
        const uint32_t bus = static_cast<uint32_t>(index);
        if (bus >= count || bus >= kMaxProcChannels)
            return kInvalidArgument;

        auto& active = dir == kInput ? inputBusActive : outputBusActive;
        const uint32_t bit = uint32_t{1} << bus;
        if (state)
            active.fetch_or(bit, std::memory_order_release);
        else
            active.fetch_and(~bit, std::memory_order_release);
        return kResultOk;
    }

    tresult setActive(TBool state) {
        if (state && g_cb && ctx) {
            g_cb->reset(ctx, sampleRate, maxFrames, procMode);
        }
        // Tell Rust the activation state for both directions so a state
        // load while suspended isn't stranded in the audio-thread queue.
        if (g_cb && ctx) {
            g_cb->set_active(ctx, state ? 1 : 0);
        }
        return kResultOk;
    }

    tresult setState(void* stream) {
        if (!stream || !g_cb || !ctx) return kResultFalse;
        int32 total = 0;
        uint8_t* data = moose_vst3_read_state_stream(stream, &total);
        tresult result = kResultFalse;
        if (data) {
            if (g_cb->state_load(ctx, data, (uint32_t)total))
                result = kResultOk;
            free(data);
        }
        return result;
    }

    tresult getState(void* stream) {
        if (!stream || !g_cb || !ctx) return kResultFalse;
        uint8_t* blob = nullptr;
        uint32_t len = 0;
        g_cb->state_save(ctx, &blob, &len);
        if (!blob) return kResultFalse;
        const int32_t ok = moose_vst3_write_state_stream(stream, blob, len);
        g_cb->state_free(blob, len);
        return ok ? kResultOk : kResultFalse;
    }

    // --- IAudioProcessor ---

    /* Our bus set is fixed: one bus per direction that has channels,
     * at the declared width. Accept a host request only when it
     * matches that shape exactly; kResultFalse tells the host to keep
     * querying getBusArrangement and use what we actually provide.
     * Blanket-accepting (the old behaviour) let a host negotiate e.g.
     * mono and then deliver buffers the plug-in never agreed to. */
    tresult setBusArrangements(uint64_t* inputs, int32 numIns, uint64_t* outputs, int32 numOuts) {
        if (!g_desc) return kResultFalse;
        auto chCount = [](uint64_t arr) {
            uint32_t c = 0;
            for (; arr; arr >>= 1) c += (uint32_t)(arr & 1);
            return c;
        };
        // The host proposes one arrangement per declared bus. Match the
        // full per-bus shape - so a sidechain bus is matched on its own
        // width, not summed - and adopt the layout it names.
        if (numIns != (int32)g_desc->num_input_buses ||
                numOuts != (int32)g_desc->num_output_buses)
            return kResultFalse;
        // Per-bus widths for the FULL declared bus count. Heap-sized, not
        // a fixed [16] the matcher would receive truncated - Rust requires
        // an exact bus-count match, so a plugin declaring more buses than a
        // local cap would advertise them yet reject every arrangement. This
        // is not the audio callback, so the allocation is free. The count
        // check above guarantees numIns/numOuts equal the descriptor's bus
        // counts.
        std::vector<uint32_t> inCh(numIns  > 0 ? (size_t)numIns  : 0);
        std::vector<uint32_t> outCh(numOuts > 0 ? (size_t)numOuts : 0);
        for (int32 i = 0; i < numIns;  i++) inCh[i]  = inputs  ? chCount(inputs[i])  : 0;
        for (int32 i = 0; i < numOuts; i++) outCh[i] = outputs ? chCount(outputs[i]) : 0;
        if (g_cb && g_cb->match_bus_layout_perbus) {
            int32_t li = g_cb->match_bus_layout_perbus(
                inCh.data(), (uint32_t)numIns, outCh.data(), (uint32_t)numOuts);
            if (li >= 0) {
                cur_layout = (uint32_t)li;
                refreshBusChannels();
                // Keep the summed cur_in/cur_out in step for the scratch
                // sizing and deactivated-bus synthesis in process().
                uint32_t si = 0, so = 0;
                for (int32 i = 0; i < numIns;  i++) si += inCh[i];
                for (int32 i = 0; i < numOuts; i++) so += outCh[i];
                cur_in = si; cur_out = so;
                return kResultOk;
            }
        }
        return kResultFalse;
    }

    tresult getBusArrangement(int32 dir, int32 index, uint64_t* arr) {
        if (!arr || !g_desc) return kInvalidArgument;
        const uint32_t busCount = (dir == kInput) ? g_desc->num_input_buses
                                                  : g_desc->num_output_buses;
        if (index < 0 || (uint32_t)index >= busCount) return kInvalidArgument;
        uint32_t ch = (g_cb && g_cb->layout_bus_channels)
            ? g_cb->layout_bus_channels(cur_layout, dir == kOutput, (uint32_t)index)
            : 0;
        if (ch == 0 || ch > 63) return kInvalidArgument;
        // Canonical SpeakerArrangement for the width (mono -> kMono, not
        // kSpeakerL; 4ch -> quad, not 3.1). getBusInfo's channelCount is
        // the popcount of this, so the two stay consistent.
        *arr = speaker_arr_for_channels(ch);
        return kResultOk;
    }

    tresult canProcessSampleSize(int32 symbolicSize) {
        if (symbolicSize == kSample32) return kResultOk;
        if (symbolicSize == kSample64 && g_desc && g_desc->supports_f64) return kResultOk;
        return kResultFalse;
    }

    uint32 getLatencySamples() {
        return (g_cb && ctx && g_cb->get_latency) ? g_cb->get_latency(ctx) : 0;
    }

    tresult setupProcessing(ProcessSetup* setup) {
        if (!setup) return kInvalidArgument;
        // Refuse a sample size we never advertised - accepting it
        // would reinterpret the host's channel pointers at the wrong
        // width in process().
        if (canProcessSampleSize(setup->symbolicSampleSize) != kResultOk)
            return kResultFalse;
        procSampleSize = setup->symbolicSampleSize;
        sampleRate = setup->sampleRate;
        maxFrames = setup->maxSamplesPerBlock;
        procMode = setup->processMode;
        // (Re)size the omitted-bus scratch for the new block bound.
        // calloc for the input side: synthesized inputs must read as
        // silence and are never written, so zeroing once here holds.
        if (g_desc) {
            size_t frameBytes = (size_t)maxFrames * sizeof(double);
            if (g_desc->num_inputs > 0 && frameBytes > 0) {
                free(silenceScratch);
                silenceScratch = (uint8_t*)calloc(1, frameBytes);
            }
            // Size the discard scratch to the WIDEST layout's output width,
            // not the first layout's (num_outputs): a negotiated wider
            // layout must still get one distinct block per output channel,
            // or two aliased discard pointers become aliased `&mut`s in the
            // Rust bridge.
            if (g_desc->max_outputs > 0 && frameBytes > 0) {
                uint32_t ch = g_desc->max_outputs > kMaxProcChannels
                                  ? kMaxProcChannels
                                  : g_desc->max_outputs;
                free(trashScratch);
                trashScratch = (uint8_t*)malloc((size_t)ch * frameBytes);
            }
        }
        return kResultOk;
    }

    tresult setProcessing(TBool) { return kResultOk; }

    tresult process(ProcessData* data) {
        if (!data || !g_cb || !ctx) return kResultOk;
        // Never reinterpret buffers at a width other than the one
        // negotiated in setupProcessing.
        if (data->symbolicSampleSize != procSampleSize) return kResultFalse;

        // Whether this call renders audio, versus a parameter flush
        // (numSamples <= 0, or a suspended effect device the host flushes
        // with no audio buses). On a flush process_chunked never runs, so
        // the shim must commit every param's block value itself; when we DO
        // process, CHUNKED params are committed per-offset by
        // process_chunked and must NOT be pre-committed here, or a mid-block
        // step is heard from sample 0.
        const bool suspendedFlush =
            g_desc && (g_desc->num_inputs > 0 || g_desc->num_outputs > 0)
            && data->numInputs == 0 && data->numOutputs == 0;
        const bool willProcess = data->numSamples > 0 && !suspendedFlush;

        // Collect ALL param change points (sample-accurate automation)
        Vst3ParamChange paramChanges[512];
        uint32_t numParamChanges = 0;

        if (data->inputParameterChanges) {
            auto** pcVtbl = (void**)*(void**)data->inputParameterChanges;
            auto getParamCount = (int32 (*)(void*))pcVtbl[3];
            auto getParamData  = (void* (*)(void*, int32))pcVtbl[4];
            int32 numChanges = getParamCount(data->inputParameterChanges);
            for (int32 i = 0; i < numChanges; i++) {
                void* queue = getParamData(data->inputParameterChanges, i);
                if (!queue) continue;
                auto** qVtbl = (void**)*(void**)queue;
                auto getParamId   = (uint32 (*)(void*))qVtbl[3];
                auto getPointCnt  = (int32 (*)(void*))qVtbl[4];
                auto getPoint     = (tresult (*)(void*, int32, int32*, double*))qVtbl[5];
                uint32 paramId = getParamId(queue);
                int32 numPoints = getPointCnt(queue);
                for (int32 j = 0; j < numPoints && numParamChanges < 512; j++) {
                    int32 sampleOffset;
                    double value;
                    if (getPoint(queue, j, &sampleOffset, &value) == kResultOk) {
                        double plain = g_cb->param_denormalize(ctx, paramId, value);
                        paramChanges[numParamChanges].id = paramId;
                        paramChanges[numParamChanges].sample_offset = sampleOffset;
                        paramChanges[numParamChanges].value = plain;
                        numParamChanges++;
                        // Block-rate commit of the queue's final value.
                        // Skipped for CHUNKED params on a processing block:
                        // process_chunked commits those at their sample
                        // offset, so pre-writing the end value here would
                        // make a mid-block step audible from sample 0. On a
                        // flush (no process_chunked) we still commit them.
                        if (j == numPoints - 1
                                && !(willProcess && g_cb->param_is_chunked(ctx, paramId)))
                            g_cb->param_set_value(ctx, paramId, plain);
                    }
                }
            }
        }

        // Extract transport info
        Vst3Transport transport = {};
        Vst3Transport* transportPtr = nullptr;
        if (data->processContext) {
            auto* pc = data->processContext;
            transport.playing = (pc->state & kPlaying) ? 1 : 0;
            transport.recording = (pc->state & kRecording) ? 1 : 0;
            transport.tempo = (pc->state & kTempoValid) ? pc->tempo : 120.0;
            transport.time_sig_num = (pc->state & kTimeSigValid) ? pc->timeSigNumerator : 4;
            transport.time_sig_den = (pc->state & kTimeSigValid) ? pc->timeSigDenominator : 4;
            transport.position_samples = (double)pc->projectTimeSamples;
            transport.position_beats = (pc->state & kProjectTimeMusicValid) ? pc->projectTimeMusic : 0.0;
            transport.bar_start_beats = (pc->state & kBarPositionValid) ? pc->barPositionMusic : 0.0;
            transport.cycle_start_beats = (pc->state & kCycleValid) ? pc->cycleStartMusic : 0.0;
            transport.cycle_end_beats = (pc->state & kCycleValid) ? pc->cycleEndMusic : 0.0;
            // kCycleActive is "loop engaged"; kCycleValid only says the
            // marker positions are readable (hosts set it with loop off).
            transport.cycle_active = (pc->state & kCycleActive) ? 1 : 0;
            transportPtr = &transport;
        }

        int32 numFrames = data->numSamples;
        if (numFrames <= 0) return kResultOk;

        /* Parameter flush: the spec allows process() with no audio
         * buses at all (numInputs == numOutputs == 0) to push
         * parameter changes while the plug-in isn't being processed,
         * and it does NOT promise numSamples == 0 for that shape -
         * Ableton Live sends nonzero numSamples when flushing a
         * suspended/off effect device. The parameter values were
         * already applied above (param_set_value per queue); running
         * DSP here would hand the plug-in a zero-channel buffer it
         * never negotiated. Only bail when the plug-in declared audio
         * buses: a pure-MIDI note effect (no audio buses by design)
         * legitimately processes every block with this shape. */
        if (suspendedFlush)
            return kResultOk;

        // Collect input/output channel pointers. channelBuffers32 /
        // channelBuffers64 are one union field; use64 records which
        // arm the negotiated sample size selects.
        const bool use64 = (procSampleSize == kSample64);
        const size_t sampleBytes = use64 ? sizeof(double) : sizeof(float);
        const void* inPtrs[kMaxProcChannels] = {};
        void* outPtrs[kMaxProcChannels] = {};
        uint32_t numIn = 0, numOut = 0;
        const uint32_t activeInputs = inputBusActive.load(std::memory_order_acquire);
        const uint32_t activeOutputs = outputBusActive.load(std::memory_order_acquire);

        // Concatenate every DECLARED bus's negotiated width into one flat
        // array in bus order. Inactive buses retain their channel positions
        // but contribute nulls, which the preallocated scratch below replaces
        // with silence/discard. Reading the declared width (rather than a
        // deactivated host buffer's often-zero numChannels) prevents a disabled
        // aux bus from shifting every later bus into the wrong flat channels.
        if (g_desc) {
            for (uint32_t b = 0; b < g_desc->num_input_buses &&
                                 b < kMaxProcChannels && numIn < kMaxProcChannels; b++) {
                const uint32_t declared = inputBusChannels[b];
                const bool active = (activeInputs & (uint32_t{1} << b)) != 0;
                const AudioBusBuffers* bus = data->inputs && data->numInputs > 0 &&
                                              b < static_cast<uint32_t>(data->numInputs)
                    ? &data->inputs[b]
                    : nullptr;
                const uint32_t supplied = bus && bus->numChannels > 0
                    ? static_cast<uint32_t>(bus->numChannels)
                    : 0;
                const bool nullArr = !bus ||
                    (use64 ? bus->channelBuffers64 == nullptr : bus->channelBuffers32 == nullptr);
                for (uint32_t c = 0; c < declared && numIn < kMaxProcChannels; c++, numIn++) {
                    inPtrs[numIn] = !active || nullArr || c >= supplied ? nullptr
                                    : use64 ? (const void*)bus->channelBuffers64[c]
                                            : (const void*)bus->channelBuffers32[c];
                }
            }
            for (uint32_t b = 0; b < g_desc->num_output_buses &&
                                 b < kMaxProcChannels && numOut < kMaxProcChannels; b++) {
                const uint32_t declared = outputBusChannels[b];
                const bool active = (activeOutputs & (uint32_t{1} << b)) != 0;
                AudioBusBuffers* bus = data->outputs && data->numOutputs > 0 &&
                                       b < static_cast<uint32_t>(data->numOutputs)
                    ? &data->outputs[b]
                    : nullptr;
                const uint32_t supplied = bus && bus->numChannels > 0
                    ? static_cast<uint32_t>(bus->numChannels)
                    : 0;
                const bool nullArr = !bus ||
                    (use64 ? bus->channelBuffers64 == nullptr : bus->channelBuffers32 == nullptr);
                for (uint32_t c = 0; c < declared && numOut < kMaxProcChannels; c++, numOut++) {
                    outPtrs[numOut] = !active || nullArr || c >= supplied ? nullptr
                                      : use64 ? (void*)bus->channelBuffers64[c]
                                              : (void*)bus->channelBuffers32[c];
                }
            }
        }

        /* Deactivated buses: the plug-in negotiated fixed widths
         * (cur_in / cur_out) and its flat AudioBuffer indexes every
         * channel. The activation mask above suppresses a disabled bus even
         * if the host supplied storage; shipping hosts may also present null
         * per-channel pointers, a null channelBuffers array, or omit inactive
         * trailing AudioBusBuffers entirely. Every shape is normalized to
         * null pointers by the gather above. Either way
         * the missing channels must read as
         * shared read-only silence (inputs) or absorb writes into
         * per-channel discard scratch (outputs), never arrive as a null
         * pointer the Rust bridge turns into an out-of-range empty
         * slice. Back-fill nulls in place (positions stay aligned since a
         * present bus still reports its width), then pad the tail up to
         * the negotiated width for dropped trailing buses. The true
         * no-bus flush shape already returned above. Skipped if a host
         * overruns its own maxSamplesPerBlock promise (the scratch is
         * sized to it). */
        if ((uint32_t)numFrames <= maxFrames) {
            if (g_desc && cur_in > 0 && silenceScratch) {
                const uint32_t wantIn = cur_in > kMaxProcChannels ? kMaxProcChannels : cur_in;
                for (uint32_t c = 0; c < numIn; c++)
                    if (!inPtrs[c]) inPtrs[c] = silenceScratch;
                for (; numIn < wantIn; numIn++)
                    inPtrs[numIn] = silenceScratch;
            }
            if (g_desc && cur_out > 0 && trashScratch) {
                const uint32_t wantOut = cur_out > kMaxProcChannels ? kMaxProcChannels : cur_out;
                // Distinct discard block per output channel - sized in
                // setupProcessing to the WIDEST layout's output width, so
                // every channel of the negotiated layout (cur_out <=
                // max_outputs) gets its own block. No two output slices
                // ever alias, which would be UB in the Rust bridge.
                const uint32_t trashCh =
                    g_desc->max_outputs > kMaxProcChannels ? kMaxProcChannels : g_desc->max_outputs;
                for (uint32_t c = 0; c < numOut; c++)
                    if (!outPtrs[c]) {
                        const uint32_t slot = (trashCh && c < trashCh) ? c : (trashCh ? trashCh - 1 : 0);
                        outPtrs[c] = trashScratch + (size_t)slot * maxFrames * sizeof(double);
                    }
                for (; numOut < wantOut; numOut++) {
                    const uint32_t slot = (trashCh && numOut < trashCh) ? numOut : (trashCh ? trashCh - 1 : 0);
                    outPtrs[numOut] = trashScratch + (size_t)slot * maxFrames * sizeof(double);
                }
            }
        }

        // Copy input to output for in-place processing
        uint32_t copyChannels = (numIn < numOut) ? numIn : numOut;
        for (uint32_t c = 0; c < copyChannels; c++) {
            if (inPtrs[c] && outPtrs[c] && inPtrs[c] != outPtrs[c])
                memcpy(outPtrs[c], inPtrs[c], numFrames * sampleBytes);
        }

        // Traverse the host list directly in its original order. Rust owns the
        // bounded queue and reports queue-full explicitly; no fixed C++ staging
        // array can silently truncate a dense block.
        if (g_cb->begin_input_events)
            g_cb->begin_input_events(ctx, (uint32_t)numFrames);

        if (data->inputEvents && g_cb->push_input_event) {
            struct IEventListVtbl {
                tresult (*qi)(void*, const TUID, void**);
                uint32 (*addRef)(void*);
                uint32 (*release)(void*);
                int32 (*getEventCount)(void*);
                tresult (*getEvent)(void*, int32, void*);
                tresult (*addEvent)(void*, void*);
            };
            struct { IEventListVtbl* vtbl; } *eventList =
                (decltype(eventList))data->inputEvents;

            const int32 eventCount = eventList->vtbl->getEventCount(eventList);
            for (int32 i = 0; i < eventCount; i++) {
                Vst3SdkEvent ev = {};
                if (eventList->vtbl->getEvent(eventList, i, &ev) != kResultOk)
                    break;

                Vst3NativeEvent native = {};
                native.kind = ev.type;
                native.sample_offset = ev.sampleOffset;
                native.bus_index = ev.busIndex;
                native.flags = ev.flags;
                native.ppq_position = ev.ppqPosition;
                native.note_id = -1;
                switch (ev.type) {
                    case kVst3NoteOnEvent:
                        native.channel = ev.noteOn.channel;
                        native.pitch = ev.noteOn.pitch;
                        native.note_id = ev.noteOn.noteId;
                        native.length = ev.noteOn.length;
                        native.velocity = ev.noteOn.velocity;
                        native.tuning = ev.noteOn.tuning;
                        break;
                    case kVst3NoteOffEvent:
                        native.channel = ev.noteOff.channel;
                        native.pitch = ev.noteOff.pitch;
                        native.note_id = ev.noteOff.noteId;
                        native.velocity = ev.noteOff.velocity;
                        native.tuning = ev.noteOff.tuning;
                        break;
                    case kVst3DataEvent:
                        native.type_id = ev.data.dataType;
                        native.bytes = ev.data.bytes;
                        native.len = ev.data.size;
                        break;
                    case kVst3PolyPressureEvent:
                        native.channel = ev.polyPressure.channel;
                        native.pitch = ev.polyPressure.pitch;
                        native.note_id = ev.polyPressure.noteId;
                        native.velocity = ev.polyPressure.pressure;
                        break;
                    case kVst3NoteExpressionValueEvent:
                        native.type_id = ev.noteExpressionValue.typeId;
                        native.note_id = ev.noteExpressionValue.noteId;
                        native.value = ev.noteExpressionValue.value;
                        break;
                    default:
                        break;
                }
                const uint32_t result = g_cb->push_input_event(ctx, &native);
                if (result == 4) // QueueFull: retain one ordered prefix.
                    break;
            }
        }

        uint32_t processOk = 0;
        if (use64)
            processOk = g_cb->process_f64(ctx, (const double**)inPtrs, (double**)outPtrs,
                                          numIn, numOut,
                                          inputBusChannels, g_desc->num_input_buses, activeInputs,
                                          outputBusChannels, g_desc->num_output_buses, activeOutputs,
                                          numFrames, transportPtr, paramChanges,
                                          numParamChanges, data->processMode);
        else
            processOk = g_cb->process(ctx, (const float**)inPtrs, (float**)outPtrs,
                                      numIn, numOut,
                                      inputBusChannels, g_desc->num_input_buses, activeInputs,
                                      outputBusChannels, g_desc->num_output_buses, activeOutputs,
                                      numFrames, transportPtr, paramChanges,
                                      numParamChanges, data->processMode);

        if (!processOk) {
            if (g_cb->finish_output_events)
                g_cb->finish_output_events(ctx, 3 /* kOutputInvalid */);
            return kResultOk;
        }

        enum OutputEventResult : uint32_t {
            kOutputEnd = 0,
            kOutputEmitted = 1,
            kOutputUnsupported = 2,
            kOutputInvalid = 3,
            kOutputQueueFull = 4,
        };
        OutputEventResult drainResult = kOutputEnd;
        const uint32_t outputParamCount = g_cb->get_output_param_count
            ? g_cb->get_output_param_count(ctx) : 0;
        if (outputParamCount > 0 && !data->outputParameterChanges)
            drainResult = kOutputUnsupported;

        // Drain exactly one globally ordered lossless stream. Rust validates
        // each event and proposes note-ID lifecycle changes; those changes are
        // committed only after the host accepts the corresponding SDK event.
        if (drainResult == kOutputEnd
                && g_cb->begin_output_events && g_cb->next_output_event
                && g_cb->commit_output_event) {
            struct OEVtbl {
                tresult (*qi)(void*, const TUID, void**);
                uint32 (*addRef)(void*);
                uint32 (*release)(void*);
                int32 (*getEventCount)(void*);
                tresult (*getEvent)(void*, int32, void*);
                tresult (*addEvent)(void*, void*);
            };
            struct { OEVtbl* vtbl; } *eventList = data->outputEvents
                ? (decltype(eventList))data->outputEvents : nullptr;

            g_cb->begin_output_events(ctx);
            for (;;) {
                Vst3NativeEvent native = {};
                const uint32_t next = g_cb->next_output_event(ctx, &native);
                if (next == kOutputEnd)
                    break;
                if (next == kOutputUnsupported || next == kOutputInvalid) {
                    drainResult = (OutputEventResult)next;
                    break;
                }
                if (next != kOutputEmitted) {
                    drainResult = next == kOutputQueueFull
                        ? kOutputQueueFull : kOutputInvalid;
                    break;
                }
                if (!eventList) {
                    drainResult = kOutputUnsupported;
                    break;
                }

                Vst3SdkEvent ev = {};
                ev.busIndex = native.bus_index;
                ev.sampleOffset = native.sample_offset;
                ev.ppqPosition = native.ppq_position;
                ev.flags = (uint16_t)native.flags;
                ev.type = (uint16_t)native.kind;
                switch (native.kind) {
                    case kVst3NoteOnEvent:
                        ev.noteOn.channel = native.channel;
                        ev.noteOn.pitch = native.pitch;
                        ev.noteOn.tuning = native.tuning;
                        ev.noteOn.velocity = native.velocity;
                        ev.noteOn.length = native.length;
                        ev.noteOn.noteId = native.note_id;
                        break;
                    case kVst3NoteOffEvent:
                        ev.noteOff.channel = native.channel;
                        ev.noteOff.pitch = native.pitch;
                        ev.noteOff.velocity = native.velocity;
                        ev.noteOff.noteId = native.note_id;
                        ev.noteOff.tuning = native.tuning;
                        break;
                    case kVst3DataEvent:
                        ev.data.size = native.len;
                        ev.data.dataType = native.type_id;
                        ev.data.bytes = native.bytes;
                        break;
                    case kVst3PolyPressureEvent:
                        ev.polyPressure.channel = native.channel;
                        ev.polyPressure.pitch = native.pitch;
                        ev.polyPressure.pressure = native.velocity;
                        ev.polyPressure.noteId = native.note_id;
                        break;
                    case kVst3NoteExpressionValueEvent:
                        ev.noteExpressionValue.typeId = native.type_id;
                        ev.noteExpressionValue.noteId = native.note_id;
                        ev.noteExpressionValue.value = native.value;
                        break;
                    case kVst3LegacyMidiCcOutEvent:
                        ev.midiCCOut.controlNumber = (uint8_t)native.type_id;
                        ev.midiCCOut.channel = (int8_t)native.channel;
                        ev.midiCCOut.value = (int8_t)native.data1;
                        ev.midiCCOut.value2 = (int8_t)native.data2;
                        break;
                    default:
                        drainResult = kOutputInvalid;
                        break;
                }
                if (drainResult == kOutputInvalid)
                    break;

                if (eventList->vtbl->addEvent(data->outputEvents, &ev) != kResultOk) {
                    drainResult = kOutputQueueFull;
                    break;
                }
                g_cb->commit_output_event(ctx);
                if (drainResult == kOutputEnd)
                    drainResult = kOutputEmitted;
            }
        }

        // Forward process-emitted parameter changes to the host's
        // output parameter queue. Hosts route these to the edit
        // controller so the UI / automation reflect values the plugin
        // changed during processing (e.g. an envelope follower). The
        // Rust side already normalized each value to [0,1].
        if (outputParamCount > 0
                && (drainResult == kOutputEnd || drainResult == kOutputEmitted)) {
            struct IPCVtbl {
                tresult (*qi)(void*, const TUID, void**);
                uint32 (*addRef)(void*);
                uint32 (*release)(void*);
                int32 (*getParameterCount)(void*);
                void* (*getParameterData)(void*, int32);
                void* (*addParameterData)(void*, const uint32*, int32*);
            };
            struct IQVtbl {
                tresult (*qi)(void*, const TUID, void**);
                uint32 (*addRef)(void*);
                uint32 (*release)(void*);
                uint32 (*getParameterId)(void*);
                int32 (*getPointCount)(void*);
                tresult (*getPoint)(void*, int32, int32*, double*);
                tresult (*addPoint)(void*, int32, double, int32*);
            };
            struct { IPCVtbl* vtbl; } *changes =
                (decltype(changes))data->outputParameterChanges;
            for (uint32_t i = 0; i < outputParamCount; i++) {
                uint32_t pid = 0;
                int32 soff = 0;
                double val = 0.0;
                g_cb->get_output_param(ctx, i, &pid, &soff, &val);
                int32 qidx = 0;
                void* queue = changes->vtbl->addParameterData(
                    data->outputParameterChanges, &pid, &qidx);
                if (!queue) {
                    drainResult = kOutputQueueFull;
                    break;
                }
                IQVtbl* qv = *(IQVtbl**)queue;
                int32 pidx = 0;
                if (qv->addPoint(queue, soff, val, &pidx) != kResultOk) {
                    drainResult = kOutputQueueFull;
                    break;
                }
                if (drainResult == kOutputEnd)
                    drainResult = kOutputEmitted;
            }
        }
        if (g_cb->finish_output_events)
            g_cb->finish_output_events(ctx, drainResult);

        return kResultOk;
    }

    uint32 getTailSamples() {
        return (g_cb && ctx && g_cb->get_tail) ? g_cb->get_tail(ctx) : 0;
    }

    // --- IEditController ---

    tresult setComponentState(void* stream) {
        // In single-component mode, the component setState already loaded
        // the params. The controller just needs to acknowledge success so
        // the host knows the controller is in sync.
        (void)stream;
        return kResultOk;
    }
    tresult setECState(void*) { return kResultOk; }
    tresult getECState(void*) { return kResultOk; }

    int32 getParameterCount() {
        return g_num_params;
    }

    tresult getParameterInfo(int32 index, ParameterInfo* info) {
        if (!info || index < 0 || (uint32_t)index >= g_num_params) return kInvalidArgument;
        auto& p = g_params[index];
        info->id = p.id;
        str_to_char16(info->title, p.name, 128);
        str_to_char16(info->shortTitle, p.short_name, 128);
        str_to_char16(info->units, p.units, 128);
        info->stepCount = p.step_count;
        info->defaultNormalizedValue = p.default_normalized;
        info->unitId = ((uint32_t)index < 4096) ? g_param_unit_id[index] : 0;
        info->flags = p.flags;
        if (g_cb && ctx) {
            char title[512] = {};
            int32 hidden = g_cb->param_presentation(ctx, p.id, title, sizeof(title));
            if (hidden >= 0) {
                str_to_char16(info->title, title, 128);
                str_to_char16(info->shortTitle, title, 128);
                // ParameterFlags: kCanAutomate=1, kIsReadOnly=2, kIsHidden=16.
                // The SDK defines hidden as read-only and not automatable.
                if (hidden) info->flags = (info->flags | 2 | 16) & ~1;
            }
        }
        return kResultOk;
    }

    tresult getParamStringByValue(uint32 id, double valueNormalized, char16* string) {
        if (!string || !g_cb || !ctx) return kInvalidArgument;
        double plain = g_cb->param_denormalize(ctx, id, valueNormalized);
        char buf[128];
        uint32_t len = g_cb->param_format(ctx, id, plain, buf, sizeof(buf));
        if (len > 0) { str_to_char16(string, buf, 128); return kResultOk; }
        char tmp[32]; snprintf(tmp, sizeof(tmp), "%.2f", plain);
        str_to_char16(string, tmp, 128);
        return kResultOk;
    }

    tresult getParamValueByString(uint32 id, char16* string, double* valueNormalized) {
        if (!string || !valueNormalized || !g_cb || !ctx) return kInvalidArgument;
        char utf8[256];
        char16_to_str(utf8, string, sizeof(utf8));
        double plain = 0.0;
        if (!g_cb->param_parse(ctx, id, utf8, &plain)) return kResultFalse;
        *valueNormalized = g_cb->param_normalize(ctx, id, plain);
        return kResultOk;
    }

    double normalizedParamToPlain(uint32 id, double normalized) {
        return (g_cb && ctx) ? g_cb->param_denormalize(ctx, id, normalized) : normalized;
    }

    double plainParamToNormalized(uint32 id, double plain) {
        return (g_cb && ctx) ? g_cb->param_normalize(ctx, id, plain) : plain;
    }

    double getParamNormalized(uint32 id) {
        // Host UI polls this on the main thread; a good place to hand off
        // a pending latency restart the audio thread flagged.
        flushPendingRestart();
        if (!g_cb || !ctx) return 0;
        double plain = g_cb->param_get_value(ctx, id);
        return g_cb->param_normalize(ctx, id, plain);
    }

    tresult setParamNormalized(uint32 id, double value) {
        if (!g_cb || !ctx) return kResultFalse;
        if (inPerformEdit) return kResultOk; // skip host→plugin echo during GUI edit
        double plain = g_cb->param_denormalize(ctx, id, value);
        g_cb->param_set_value(ctx, id, plain);
        return kResultOk;
    }

    tresult setComponentHandler(void* handler) {
        if (componentHandler) {
            auto release_fn = (uint32 (*)(void*))(*(void***)componentHandler)[2];
            release_fn(componentHandler);
        }
        componentHandler = handler;
        if (componentHandler) {
            auto addRef_fn = (uint32 (*)(void*))(*(void***)componentHandler)[1];
            addRef_fn(componentHandler);
        }
        return kResultOk;
    }

    // Drain any restart bits the audio thread flagged, calling
    // IComponentHandler::restartComponent. Only ever invoked from host
    // main-thread callbacks (param reads, edit gestures), so the call
    // lands on the UI thread as VST3 wants. IComponentHandler vtable:
    //   [3] beginEdit [4] performEdit [5] endEdit [6] restartComponent
    uint64_t presentationRevision = 0;
    uint64_t busNamesRevision = 0;
    void flushPendingRestart() {
        if (!componentHandler) return;
        uint64_t revision = (g_cb && ctx) ? g_cb->param_presentation_revision(ctx) : 0;
        if (revision != presentationRevision) {
            presentationRevision = revision;
            pendingRestart.fetch_or(1 << 4, std::memory_order_release); // kParamTitlesChanged
        }
        uint64_t names = (g_cb && ctx && g_cb->output_bus_names_revision)
            ? g_cb->output_bus_names_revision(ctx) : 0;
        if (names != busNamesRevision) {
            busNamesRevision = names;
            pendingRestart.fetch_or(1 << 7, std::memory_order_release); // kIoTitlesChanged
        }
        int32 flags = pendingRestart.exchange(0, std::memory_order_acq_rel);
        if (!flags || !componentHandler) return;
        auto restart = (tresult (*)(void*, int32))(*(void***)componentHandler)[6];
        restart(componentHandler, flags);
    }

#if defined(__linux__)
    // (Re)bind the restart timer to the host's run loop for the given
    // plug frame. Called from pv_setFrame / view release (frame or
    // nullptr) and the destructor (nullptr). Idempotent: it always drops
    // any prior registration first, so a host handing us a fresh frame
    // (Cubase theme change, Live dock/undock) or a null frame (view
    // detached) rebinds cleanly. Runs on the host UI thread, same as
    // onTimer, so there's no race with the handler.
    void updateRunLoopRegistration(void* frame) {
        if (runLoop) {
            auto unreg = (tresult (*)(void*, void*))
                (*(void***)runLoop)[kRunLoopUnregisterTimerIndex];
            unreg(runLoop, &restartTimer);
            auto rel = (uint32 (*)(void*))(*(void***)runLoop)[2];
            rel(runLoop);
            runLoop = nullptr;
        }
        if (!frame) return;
        // The host exposes its UI-thread run loop through the plug frame.
        // Not every host implements it (then the query fails and we keep
        // relying on the param-poll / edit-gesture flush, same as
        // Windows). queryInterface hands back an addRef'd pointer we keep
        // until unregister.
        void* rl = nullptr;
        auto qi = (tresult (*)(void*, const TUID, void**))(*(void***)frame)[0];
        if (qi(frame, IRunLoop_iid, &rl) != kResultOk || !rl) return;
        auto reg = (tresult (*)(void*, void*, uint64_t))
            (*(void***)rl)[kRunLoopRegisterTimerIndex];
        if (reg(rl, &restartTimer, kRestartTimerIntervalMs) == kResultOk) {
            runLoop = rl;
        } else {
            auto rel = (uint32 (*)(void*))(*(void***)rl)[2];
            rel(rl);
        }
    }
#endif

    void* createView(FIDString /*name*/, void* owner);
};

#if defined(__linux__)
// ITimerHandler method bodies - defined here, now that MooseComponent is
// complete. The handler is embedded in the component and not
// independently owned, so addRef / release are inert and queryInterface
// hands back the same pointer for FUnknown / ITimerHandler.
static tresult th_queryInterface(void* self, const TUID iid, void** obj) {
    if (iid_equal(iid, FUnknown_iid) || iid_equal(iid, ITimerHandler_iid)) {
        *obj = self;
        return kResultOk;
    }
    *obj = nullptr;
    return kNoInterface;
}
static uint32 th_addRef(void*) { return 1; }
static uint32 th_release(void*) { return 1; }
static void th_onTimer(void* self) {
    // Runs on the host UI thread every kRestartTimerIntervalMs while an
    // editor is open. Cheap when idle: flushPendingRestart exchanges the
    // atomic and returns immediately when no bits are set.
    auto* h = (RestartTimerHandler*)self;
    if (h->comp) h->comp->flushPendingRestart();
}
#endif

#if defined(_WIN32)
// Restart-timer TIMERPROC (declared up top). Dispatched by the message
// pump of the thread that created the timer - the host UI thread - every
// kRestartTimerIntervalMs for the component's whole life. Resolves the
// component through the live-instance map by timer id (the destructor
// unregisters from the map before KillTimer, on this same thread), so a
// stale WM_TIMER never touches a freed instance. Cheap when idle:
// flushPendingRestart exchanges the atomic and returns immediately when
// no bits are set.
static void CALLBACK moose_vst3_restart_timer_proc(
        HWND, UINT, UINT_PTR id, DWORD) {
    for (int i = 0; i < kMaxInstances; i++) {
        MooseComponent* comp = g_ctx_map_comp[i];
        if (comp && comp->restartTimerId == id) {
            comp->flushPendingRestart();
            return;
        }
    }
}
#endif

// ---------------------------------------------------------------------------
// IPlugView - minimal COM object for GUI embedding
// ---------------------------------------------------------------------------

struct IPlugViewVtbl {
    tresult (*queryInterface)(void*, const TUID, void**);
    uint32  (*addRef)(void*);
    uint32  (*release)(void*);
    tresult (*isPlatformTypeSupported)(void*, FIDString);
    tresult (*attached)(void*, void*, FIDString);
    tresult (*removed)(void*);
    tresult (*onWheel)(void*, float);
    tresult (*onKeyDown)(void*, char16, int16_t, int16_t);
    tresult (*onKeyUp)(void*, char16, int16_t, int16_t);
    tresult (*getSize)(void*, void*);  // ViewRect*
    tresult (*onSize)(void*, void*);
    tresult (*onFocus)(void*, int8);
    tresult (*setFrame)(void*, void*);
    tresult (*canResize)(void*);
    tresult (*checkSizeConstraint)(void*, void*);
};

struct ViewRect { int32 left; int32 top; int32 right; int32 bottom; };

// IPlugViewContentScaleSupport - one method past FUnknown.
struct IPlugViewContentScaleSupportVtbl {
    tresult (*queryInterface)(void*, const TUID, void**);
    uint32  (*addRef)(void*);
    uint32  (*release)(void*);
    tresult (*setContentScaleFactor)(void*, float);
};

// Two-vtable layout so IPlugView and IPlugViewContentScaleSupport share
// a single COM object. queryInterface returns a pointer to the relevant
// vtable slot; the functions behind each vtable derive the base with
// pointer arithmetic, matching the MooseComponent pattern.
static uint32 comp_release(void* s);
static tresult pv_setFrame(void* s, void* frame);

struct MoosePlugView {
    IPlugViewVtbl* vtbl;
    IPlugViewContentScaleSupportVtbl* vtbl_scale;
    int32_t refCount;
    void* ctx;              // Rust plugin context
    MooseComponent* comp;   // owning component (resize and run-loop lifetime)
    void* owner;            // retained component interface; released with the view
    // Stored by `pv_setFrame` (host hands the plug-view its
    // `IPlugFrame*`). Used by `moose_vst3_request_resize` to drive
    // the plugin -> host resize request. Nulled when the view is
    // released so a stale frame pointer can't survive a view
    // re-creation across theme change / dock-undock in Cubase /
    // Live.
    void* frame;
};

static MoosePlugView* pv_from_scale(void* s) {
    return reinterpret_cast<MoosePlugView*>(
        reinterpret_cast<char*>(s) - sizeof(void*));
}

static tresult pv_queryInterface(void* s, const TUID iid, void** obj) {
    auto* pv = (MoosePlugView*)s;
    if (iid_equal(iid, FUnknown_iid) || iid_equal(iid, IPlugView_iid)) {
        pv->refCount++;
        *obj = &pv->vtbl;
        return kResultOk;
    }
    if (iid_equal(iid, IPlugViewContentScaleSupport_iid)) {
        pv->refCount++;
        *obj = &pv->vtbl_scale;
        return kResultOk;
    }
    *obj = nullptr;
    return kNoInterface;
}
static uint32 pv_addRef(void* s) { return ++((MoosePlugView*)s)->refCount; }
static uint32 pv_release(void* s) {
    auto* pv = (MoosePlugView*)s;
    if (--pv->refCount <= 0) {
        if (pv->comp) {
#if defined(__linux__)
            // The frame goes away with the view: drop the run-loop
            // registration so the host doesn't hold a handler tied to a
            // freed view. A spec-compliant host already called
            // setFrame(nullptr); this covers those that don't.
            pv->comp->updateRunLoopRegistration(nullptr);
#endif
            // Null the component's pointer to this view so
            // `moose_vst3_request_resize` doesn't dereference a
            // freed plug-view between the host releasing the
            // editor and creating a new one. The view retains the component
            // until its own final release, independently of host references.
            if (pv->comp->plugView == pv) {
                pv->comp->plugView = nullptr;
            }
        }
        // Release the retained host frame after unregistering its run loop.
        pv_setFrame(pv, nullptr);
        auto* owner = pv->owner;
        free(pv);
        if (owner) comp_release(owner);
        return 0;
    }
    return pv->refCount;
}

// --- IPlugViewContentScaleSupport methods (second vtable) ---
static tresult pvcs_queryInterface(void* s, const TUID iid, void** obj) {
    return pv_queryInterface(pv_from_scale(s), iid, obj);
}
static uint32 pvcs_addRef(void* s) { return pv_addRef(pv_from_scale(s)); }
static uint32 pvcs_release(void* s) { return pv_release(pv_from_scale(s)); }
static tresult pvcs_setContentScaleFactor(void* s, float factor);  // forward

static IPlugViewContentScaleSupportVtbl g_plugview_scale_vtbl = {
    pvcs_queryInterface, pvcs_addRef, pvcs_release,
    pvcs_setContentScaleFactor,
};

static tresult pvcs_setContentScaleFactor(void* s, float factor) {
    auto* pv = pv_from_scale(s);
    if (!g_cb || !pv->ctx || !g_cb->gui_set_content_scale) return kResultFalse;
    g_cb->gui_set_content_scale(pv->ctx, (double)factor);
    return kResultOk;
}
static tresult pv_isPlatformTypeSupported(void*, FIDString type) {
    #ifdef __APPLE__
    if (strcmp(type, kPlatformTypeNSView) == 0) return kResultOk;
    #elif defined(_WIN32)
    if (strcmp(type, kPlatformTypeHWND) == 0) return kResultOk;
    #elif defined(__linux__)
    if (strcmp(type, kPlatformTypeX11) == 0) return kResultOk;
    #endif
    return kResultFalse;
}
static tresult pv_attached(void* s, void* parent, FIDString /*type*/) {
    auto* pv = (MoosePlugView*)s;
    if (!g_cb || !pv->ctx) return kResultOk;
    // KONTAKTO patch: IPlugView attachment creates the view independently
    // of processor activation or state restoration. An inactive fresh
    // instance may receive neither; deferring here leaves only a host frame.
    g_cb->gui_open(pv->ctx, parent);
    return kResultOk;
}
static tresult pv_removed(void* s) {
    auto* pv = (MoosePlugView*)s;
    if (g_cb && pv->ctx) g_cb->gui_close(pv->ctx);
    return kResultOk;
}
static tresult pv_getSize(void* s, void* rect) {
    auto* pv = (MoosePlugView*)s;
    auto* r = (ViewRect*)rect;
    if (g_cb && pv->ctx) {
        uint32_t w = 0, h = 0;
        g_cb->gui_get_size(pv->ctx, &w, &h);
        r->left = 0; r->top = 0;
        r->right = (int32)w; r->bottom = (int32)h;
        return kResultOk;
    }
    return kResultFalse;
}
static tresult pv_stub1(void*, float) { return kResultFalse; }
static tresult pv_stub2(void*, char16, int16_t, int16_t) { return kResultFalse; }
static tresult pv_stub4(void*, int8) { return kResultFalse; }

// `IPlugView::onSize`. Host commits a new size; delegate to Rust
// so the editor's `set_size` runs. VST3 doesn't have a
// "no thanks, paint at the old size" path on onSize (the host has
// already committed), so we always return kResultOk regardless of
// what `Editor::set_size` says.
static tresult pv_onSize(void* s, void* rect) {
    auto* pv = (MoosePlugView*)s;
    auto* r = (ViewRect*)rect;
    if (g_cb && pv->ctx && g_cb->gui_set_size && r) {
        int32 w = r->right - r->left;
        int32 h = r->bottom - r->top;
        if (w > 0 && h > 0) {
            g_cb->gui_set_size(pv->ctx, (uint32_t)w, (uint32_t)h);
        }
    }
    return kResultOk;
}

// `IPlugView::setFrame`. Host hands the plug-view its frame; we
// stash the pointer so `moose_vst3_request_resize` can drive a
// plugin-initiated resize through it.
static tresult pv_setFrame(void* s, void* frame) {
    auto* pv = (MoosePlugView*)s;
    if (pv->frame == frame) return kResultOk;
    if (frame) {
        auto add_ref = (uint32 (*)(void*))(*(void***)frame)[1];
        add_ref(frame);
    }
    auto* previous = pv->frame;
    pv->frame = frame;
#if defined(__linux__)
    // Bind (or, on a null frame, unbind) the restart timer to the frame's
    // run loop so a latency change flagged by the audio thread reaches the
    // host UI thread even without param polling.
    if (pv->comp) pv->comp->updateRunLoopRegistration(frame);
#endif
    if (previous) {
        auto release = (uint32 (*)(void*))(*(void***)previous)[2];
        release(previous);
    }
    return kResultOk;
}

// `IPlugView::canResize`. Returns kResultTrue when the editor
// opts in to host-driven resize, kResultFalse otherwise.
static tresult pv_canResize(void* s) {
    auto* pv = (MoosePlugView*)s;
    if (!g_cb || !pv->ctx || !g_cb->gui_can_resize) return kResultFalse;
    return g_cb->gui_can_resize(pv->ctx) != 0 ? kResultOk : kResultFalse;
}

// `IPlugView::checkSizeConstraint`. CLAMP the requested rect to
// the editor's bounds; always return kResultOk because VST3
// defines kResultFalse as "rect unrecoverable", not "I'd prefer
// not". Ableton Live calls this even after `canResize ==
// kResultFalse`; for fixed editors we snap to the editor's
// current size, matching JUCE's pattern.
static tresult pv_checkSizeConstraint(void* s, void* rect) {
    auto* pv = (MoosePlugView*)s;
    auto* r = (ViewRect*)rect;
    if (!r || !g_cb || !pv->ctx) return kResultOk;
    int32 w = r->right - r->left;
    int32 h = r->bottom - r->top;
    if (w <= 0 || h <= 0) return kResultOk;
    if (g_cb->gui_check_size_constraint) {
        uint32_t uw = (uint32_t)w;
        uint32_t uh = (uint32_t)h;
        g_cb->gui_check_size_constraint(pv->ctx, &uw, &uh);
        r->right = r->left + (int32)uw;
        r->bottom = r->top + (int32)uh;
    }
    return kResultOk;
}

static IPlugViewVtbl g_plugview_vtbl = {
    pv_queryInterface, pv_addRef, pv_release,
    pv_isPlatformTypeSupported,
    pv_attached, pv_removed,
    pv_stub1,            // onWheel
    pv_stub2,            // onKeyDown
    pv_stub2,            // onKeyUp
    pv_getSize,
    pv_onSize,
    pv_stub4,            // onFocus
    pv_setFrame,
    pv_canResize,
    pv_checkSizeConstraint,
};

// ---------------------------------------------------------------------------
// Vtables - laid out exactly as C++ COM hosts expect
// ---------------------------------------------------------------------------

// We use a multi-vtable approach: the object has 3 vtable pointers at the start.
// queryInterface returns different offsets for different IIDs.
// This is the standard C++ multiple-inheritance COM layout.

struct IComponentVtbl {
    tresult (*queryInterface)(void*, const TUID, void**);
    uint32  (*addRef)(void*);
    uint32  (*release)(void*);
    tresult (*initialize)(void*, void*);
    tresult (*terminate)(void*);
    tresult (*getControllerClassId)(void*, TUID);
    tresult (*setIoMode)(void*, int32);
    int32   (*getBusCount)(void*, int32, int32);
    tresult (*getBusInfo)(void*, int32, int32, int32, BusInfo*);
    tresult (*getRoutingInfo)(void*, RoutingInfo*, RoutingInfo*);
    tresult (*activateBus)(void*, int32, int32, int32, TBool);
    tresult (*setActive)(void*, TBool);
    tresult (*setState)(void*, void*);
    tresult (*getState)(void*, void*);
};

struct IAudioProcessorVtbl {
    tresult (*queryInterface)(void*, const TUID, void**);
    uint32  (*addRef)(void*);
    uint32  (*release)(void*);
    tresult (*setBusArrangements)(void*, uint64_t*, int32, uint64_t*, int32);
    tresult (*getBusArrangement)(void*, int32, int32, uint64_t*);
    tresult (*canProcessSampleSize)(void*, int32);
    uint32  (*getLatencySamples)(void*);
    tresult (*setupProcessing)(void*, ProcessSetup*);
    tresult (*setProcessing)(void*, TBool);
    tresult (*process)(void*, ProcessData*);
    uint32  (*getTailSamples)(void*);
};

struct IEditControllerVtbl {
    tresult (*queryInterface)(void*, const TUID, void**);
    uint32  (*addRef)(void*);
    uint32  (*release)(void*);
    tresult (*initialize)(void*, void*);
    tresult (*terminate)(void*);
    tresult (*setComponentState)(void*, void*);
    tresult (*setState)(void*, void*);
    tresult (*getState)(void*, void*);
    int32   (*getParameterCount)(void*);
    tresult (*getParameterInfo)(void*, int32, ParameterInfo*);
    tresult (*getParamStringByValue)(void*, uint32, double, char16*);
    tresult (*getParamValueByString)(void*, uint32, char16*, double*);
    double  (*normalizedParamToPlain)(void*, uint32, double);
    double  (*plainParamToNormalized)(void*, uint32, double);
    double  (*getParamNormalized)(void*, uint32);
    tresult (*setParamNormalized)(void*, uint32, double);
    tresult (*setComponentHandler)(void*, void*);
    void*   (*createView)(void*, FIDString);
};

struct IEditControllerHostEditingVtbl {
    tresult (*queryInterface)(void*, const TUID, void**);
    uint32  (*addRef)(void*);
    uint32  (*release)(void*);
    tresult (*beginEditFromHost)(void*, uint32);
    tresult (*endEditFromHost)(void*, uint32);
};

struct IMidiMappingVtbl {
    tresult (*queryInterface)(void*, const TUID, void**);
    uint32  (*addRef)(void*);
    uint32  (*release)(void*);
    tresult (*getMidiControllerAssignment)(void*, int32, int16_t, int16_t, uint32*);
};

struct IProcessContextRequirementsVtbl {
    tresult (*queryInterface)(void*, const TUID, void**);
    uint32  (*addRef)(void*);
    uint32  (*release)(void*);
    uint32  (*getProcessContextRequirements)(void*);
};

struct UnitInfo {
    int32 id;
    int32 parentUnitId;
    char16 name[128];
    int32 programListId; // -1 = no program list
};

struct IUnitInfoVtbl {
    tresult (*queryInterface)(void*, const TUID, void**);
    uint32  (*addRef)(void*);
    uint32  (*release)(void*);
    int32   (*getUnitCount)(void*);
    tresult (*getUnitInfo)(void*, int32, UnitInfo*);
    int32   (*getProgramListCount)(void*);
    tresult (*getProgramListInfo)(void*, int32, void*);
    tresult (*getProgramName)(void*, int32, int32, char16*);
    tresult (*getProgramInfo)(void*, int32, int32, const char*, char16*);
    tresult (*hasProgramPitchNames)(void*, int32, int32);
    tresult (*getProgramPitchName)(void*, int32, int32, int16_t, char16*);
    int32   (*getSelectedUnit)(void*);
    tresult (*selectUnit)(void*, int32);
    tresult (*getUnitByBus)(void*, int32, int32, int32, int32, int32*);
    tresult (*setUnitProgramData)(void*, int32, int32, void*);
};

// Structs from the SDK's ivstnoteexpression.h. Natural alignment
// matches the SDK's packing on every 64-bit target we build for
// (the SDK packs to 16, which is >= the largest member alignment).
struct NoteExpressionValueDescription {
    double defaultValue;
    double minimum;
    double maximum;
    int32 stepCount;
};

struct NoteExpressionTypeInfo {
    uint32 typeId;
    char16 title[128];
    char16 shortTitle[128];
    char16 units[128];
    int32 unitId;
    NoteExpressionValueDescription valueDesc;
    uint32 associatedParameterId;
    int32 flags;
};
static_assert(sizeof(NoteExpressionTypeInfo) == 816,
              "must match the SDK's packed layout");

struct INoteExpressionControllerVtbl {
    tresult (*queryInterface)(void*, const TUID, void**);
    uint32  (*addRef)(void*);
    uint32  (*release)(void*);
    int32   (*getNoteExpressionCount)(void*, int32, int16_t);
    tresult (*getNoteExpressionInfo)(void*, int32, int16_t, int32, NoteExpressionTypeInfo*);
    tresult (*getNoteExpressionStringByValue)(void*, int32, int16_t, uint32, double, char16*);
    tresult (*getNoteExpressionValueByString)(void*, int32, int16_t, uint32, const char16*, double*);
};

// The actual COM object layout: one pointer per implemented interface
// vtable, followed by the C++ object. `com_from_*` offsets index into
// this pointer block, so new interfaces append before `impl` only.
struct MooseComponentCOM {
    IComponentVtbl* vtbl_component;
    IAudioProcessorVtbl* vtbl_processor;
    IEditControllerVtbl* vtbl_controller;
    IEditControllerHostEditingVtbl* vtbl_host_editing;
    IMidiMappingVtbl* vtbl_midimapping;
    IProcessContextRequirementsVtbl* vtbl_pcr;
    IUnitInfoVtbl* vtbl_unitinfo;
    INoteExpressionControllerVtbl* vtbl_note_expression;
    MooseComponent impl;
};

// Deferred: createView
void* MooseComponent::createView(FIDString /*name*/, void* owner) {
    if (!g_cb || !ctx) return nullptr;
    if (!g_cb->gui_has_editor(ctx)) return nullptr;
    auto* pv = (MoosePlugView*)calloc(1, sizeof(MoosePlugView));
    if (!pv) return nullptr;
    // Editor callbacks keep ctx/comp alive even after the host releases its
    // controller/component interfaces. Match the SDK EditorView ownership.
    addRef();
    pv->vtbl = &g_plugview_vtbl;
    pv->vtbl_scale = &g_plugview_scale_vtbl;
    pv->refCount = 1;
    pv->ctx = ctx;
    pv->comp = this;
    pv->owner = owner;
    pv->frame = nullptr;
    // Track the live plug view on the component so
    // `moose_vst3_request_resize` can find it without holding a
    // raw pointer in a Rust closure. Replacing an earlier
    // outstanding view is fine: VST3 hosts use one editor at a
    // time, and the most-recently-created is the one
    // request_resize should target.
    plugView = pv;
    return pv;
}

// Deferred definition of queryInterface (needs complete MooseComponentCOM)
tresult MooseComponent::queryInterface(void* comBase, const TUID iid, void** obj) {
    auto* com = reinterpret_cast<MooseComponentCOM*>(comBase);
    if (iid_equal(iid, FUnknown_iid) ||
        iid_equal(iid, IPluginBase_iid) ||
        iid_equal(iid, IComponent_iid)) {
        addRef();
        *obj = &com->vtbl_component;
        return kResultOk;
    }
    if (iid_equal(iid, IAudioProcessor_iid)) {
        addRef();
        *obj = &com->vtbl_processor;
        return kResultOk;
    }
    if (iid_equal(iid, IEditController_iid)) {
        addRef();
        *obj = &com->vtbl_controller;
        return kResultOk;
    }
    if (iid_equal(iid, IEditControllerHostEditing_iid)) {
        addRef();
        *obj = &com->vtbl_host_editing;
        return kResultOk;
    }
    if (iid_equal(iid, IMidiMapping_iid)) {
        addRef();
        *obj = &com->vtbl_midimapping;
        return kResultOk;
    }
    if (iid_equal(iid, IProcessContextRequirements_iid)) {
        addRef();
        *obj = &com->vtbl_pcr;
        return kResultOk;
    }
    if (iid_equal(iid, IUnitInfo_iid) && g_num_units > 1) {
        addRef();
        *obj = &com->vtbl_unitinfo;
        return kResultOk;
    }
    // Only MIDI-taking plugins accept note expression. Cubase-family
    // hosts probe this interface to decide whether to offer (and send)
    // note-expression input at all.
    if (iid_equal(iid, INoteExpressionController_iid) &&
        g_desc && g_desc->midi_input_ports > 0) {
        addRef();
        *obj = &com->vtbl_note_expression;
        return kResultOk;
    }
    *obj = nullptr;
    return kNoInterface;
}

// Helper to get MooseComponentCOM from any vtable pointer
static MooseComponentCOM* com_from_component(void* self) {
    return reinterpret_cast<MooseComponentCOM*>(self);
}
static MooseComponentCOM* com_from_processor(void* self) {
    return reinterpret_cast<MooseComponentCOM*>(
        reinterpret_cast<char*>(self) - sizeof(void*));
}
static MooseComponentCOM* com_from_controller(void* self) {
    return reinterpret_cast<MooseComponentCOM*>(
        reinterpret_cast<char*>(self) - 2 * sizeof(void*));
}
static MooseComponentCOM* com_from_host_editing(void* self) {
    return reinterpret_cast<MooseComponentCOM*>(
        reinterpret_cast<char*>(self) - 3 * sizeof(void*));
}
static MooseComponentCOM* com_from_midimapping(void* self) {
    return reinterpret_cast<MooseComponentCOM*>(
        reinterpret_cast<char*>(self) - 4 * sizeof(void*));
}
static MooseComponentCOM* com_from_pcr(void* self) {
    return reinterpret_cast<MooseComponentCOM*>(
        reinterpret_cast<char*>(self) - 5 * sizeof(void*));
}
static MooseComponentCOM* com_from_unitinfo(void* self) {
    return reinterpret_cast<MooseComponentCOM*>(
        reinterpret_cast<char*>(self) - 6 * sizeof(void*));
}
static MooseComponentCOM* com_from_note_expression(void* self) {
    return reinterpret_cast<MooseComponentCOM*>(
        reinterpret_cast<char*>(self) - 7 * sizeof(void*));
}

// --- Component vtable functions ---
#define COMP(self) (com_from_component(self)->impl)
static tresult comp_qi(void* s, const TUID iid, void** obj) { return COMP(s).queryInterface(com_from_component(s), iid, obj); }
static uint32 comp_addRef(void* s) { return COMP(s).addRef(); }
static uint32 comp_release(void* s) { auto* com = com_from_component(s); auto r = com->impl.release(); if (r == 0) { com->impl.~MooseComponent(); free(com); } return r; }
static tresult comp_init(void* s, void* ctx) { return COMP(s).initialize(ctx); }
static tresult comp_term(void* s) { return COMP(s).terminate(); }
static tresult comp_getCtrlId(void* s, TUID id) { return COMP(s).getControllerClassId(id); }
static tresult comp_setIoMode(void* s, int32 m) { return COMP(s).setIoMode(m); }
static int32 comp_getBusCount(void* s, int32 t, int32 d) { return COMP(s).getBusCount(t, d); }
static tresult comp_getBusInfo(void* s, int32 t, int32 d, int32 i, BusInfo* b) { return COMP(s).getBusInfo(t, d, i, b); }
static tresult comp_getRouting(void* s, RoutingInfo* a, RoutingInfo* b) { return COMP(s).getRoutingInfo(a, b); }
static tresult comp_activateBus(void* s, int32 t, int32 d, int32 i, TBool st) { return COMP(s).activateBus(t, d, i, st); }
static tresult comp_setActive(void* s, TBool st) { return COMP(s).setActive(st); }
static tresult comp_setState(void* s, void* st) { return COMP(s).setState(st); }
static tresult comp_getState(void* s, void* st) { return COMP(s).getState(st); }

// --- Processor vtable functions ---
#define PROC(self) (com_from_processor(self)->impl)
static tresult proc_qi(void* s, const TUID iid, void** obj) { return PROC(s).queryInterface(com_from_processor(s), iid, obj); }
static uint32 proc_addRef(void* s) { return PROC(s).addRef(); }
static uint32 proc_release(void* s) { auto* com = com_from_processor(s); auto r = com->impl.release(); if (r == 0) { com->impl.~MooseComponent(); free(com); } return r; }
static tresult proc_setBusArr(void* s, uint64_t* a, int32 b, uint64_t* c, int32 d) { return PROC(s).setBusArrangements(a, b, c, d); }
static tresult proc_getBusArr(void* s, int32 d, int32 i, uint64_t* a) { return PROC(s).getBusArrangement(d, i, a); }
static tresult proc_canProcess(void* s, int32 ss) { return PROC(s).canProcessSampleSize(ss); }
static uint32 proc_getLatency(void* s) { return PROC(s).getLatencySamples(); }
static tresult proc_setup(void* s, ProcessSetup* p) { return PROC(s).setupProcessing(p); }
static tresult proc_setProc(void* s, TBool st) { return PROC(s).setProcessing(st); }
static tresult proc_process(void* s, ProcessData* d) { return PROC(s).process(d); }
static uint32 proc_getTail(void* s) { return PROC(s).getTailSamples(); }

// --- Controller vtable functions ---
#define CTRL(self) (com_from_controller(self)->impl)
static tresult ctrl_qi(void* s, const TUID iid, void** obj) { return CTRL(s).queryInterface(com_from_controller(s), iid, obj); }
static uint32 ctrl_addRef(void* s) { return CTRL(s).addRef(); }
static uint32 ctrl_release(void* s) { auto* com = com_from_controller(s); auto r = com->impl.release(); if (r == 0) { com->impl.~MooseComponent(); free(com); } return r; }
static tresult ctrl_init(void* s, void* ctx) { return CTRL(s).initialize(ctx); }
static tresult ctrl_term(void* s) { return CTRL(s).terminate(); }
static tresult ctrl_setCompState(void* s, void* st) { return CTRL(s).setComponentState(st); }
static tresult ctrl_setState(void* s, void* st) { return CTRL(s).setECState(st); }
static tresult ctrl_getState(void* s, void* st) { return CTRL(s).getECState(st); }
static int32 ctrl_getParamCount(void* s) { return CTRL(s).getParameterCount(); }
static tresult ctrl_getParamInfo(void* s, int32 i, ParameterInfo* p) { return CTRL(s).getParameterInfo(i, p); }
static tresult ctrl_getParamStr(void* s, uint32 id, double v, char16* str) { return CTRL(s).getParamStringByValue(id, v, str); }
static tresult ctrl_getParamVal(void* s, uint32 id, char16* str, double* v) { return CTRL(s).getParamValueByString(id, str, v); }
static double ctrl_n2p(void* s, uint32 id, double v) { return CTRL(s).normalizedParamToPlain(id, v); }
static double ctrl_p2n(void* s, uint32 id, double v) { return CTRL(s).plainParamToNormalized(id, v); }
static double ctrl_getPN(void* s, uint32 id) { return CTRL(s).getParamNormalized(id); }
static tresult ctrl_setPN(void* s, uint32 id, double v) { return CTRL(s).setParamNormalized(id, v); }
static tresult ctrl_setHandler(void* s, void* h) { return CTRL(s).setComponentHandler(h); }
static void* ctrl_createView(void* s, FIDString n) { return CTRL(s).createView(n, com_from_controller(s)); }

// --- HostEditing vtable functions ---
#define HEDIT(self) (com_from_host_editing(self)->impl)
static tresult hedit_qi(void* s, const TUID iid, void** obj) { return HEDIT(s).queryInterface(com_from_host_editing(s), iid, obj); }
static uint32 hedit_addRef(void* s) { return HEDIT(s).addRef(); }
static uint32 hedit_release(void* s) { auto* com = com_from_host_editing(s); auto r = com->impl.release(); if (r == 0) { com->impl.~MooseComponent(); free(com); } return r; }
static tresult hedit_beginEditFromHost(void*, uint32) { return kResultOk; }
static tresult hedit_endEditFromHost(void*, uint32) { return kResultOk; }

// --- MidiMapping vtable functions ---
#define MMAP(self) (com_from_midimapping(self)->impl)
static tresult mmap_qi(void* s, const TUID iid, void** obj) { return MMAP(s).queryInterface(com_from_midimapping(s), iid, obj); }
static uint32 mmap_addRef(void* s) { return MMAP(s).addRef(); }
static uint32 mmap_release(void* s) { auto* com = com_from_midimapping(s); auto r = com->impl.release(); if (r == 0) { com->impl.~MooseComponent(); free(com); } return r; }
// The Rust resolver also owns the per-instance collision-free hidden
// proxy ID table. Returns 1 on a hit, which maps to kResultOk.
static tresult mmap_getAssignment(void* s, int32 busIndex, int16_t channel, int16_t cc, uint32* id) {
    if (!g_cb || !g_cb->midi_mapping_get_param_id || !id) return kResultFalse;
    return g_cb->midi_mapping_get_param_id(MMAP(s).rustContext(), busIndex, channel, cc, id) ? kResultOk : kResultFalse;
}

// Static vtables
static IComponentVtbl g_comp_vtbl = {
    comp_qi, comp_addRef, comp_release, comp_init, comp_term,
    comp_getCtrlId, comp_setIoMode, comp_getBusCount, comp_getBusInfo,
    comp_getRouting, comp_activateBus, comp_setActive, comp_setState, comp_getState
};

static IAudioProcessorVtbl g_proc_vtbl = {
    proc_qi, proc_addRef, proc_release,
    proc_setBusArr, proc_getBusArr, proc_canProcess, proc_getLatency,
    proc_setup, proc_setProc, proc_process, proc_getTail
};

static IEditControllerVtbl g_ctrl_vtbl = {
    ctrl_qi, ctrl_addRef, ctrl_release, ctrl_init, ctrl_term,
    ctrl_setCompState, ctrl_setState, ctrl_getState,
    ctrl_getParamCount, ctrl_getParamInfo, ctrl_getParamStr, ctrl_getParamVal,
    ctrl_n2p, ctrl_p2n, ctrl_getPN, ctrl_setPN, ctrl_setHandler, ctrl_createView
};

static IEditControllerHostEditingVtbl g_hedit_vtbl = {
    hedit_qi, hedit_addRef, hedit_release,
    hedit_beginEditFromHost, hedit_endEditFromHost
};

static IMidiMappingVtbl g_mmap_vtbl = {
    mmap_qi, mmap_addRef, mmap_release, mmap_getAssignment
};

// --- ProcessContextRequirements vtable functions ---
#define PCR(self) (com_from_pcr(self)->impl)
static tresult pcr_qi(void* s, const TUID iid, void** obj) { return PCR(s).queryInterface(com_from_pcr(s), iid, obj); }
static uint32 pcr_addRef(void* s) { return PCR(s).addRef(); }
static uint32 pcr_release(void* s) { auto* com = com_from_pcr(s); auto r = com->impl.release(); if (r == 0) { com->impl.~MooseComponent(); free(com); } return r; }
static uint32 pcr_getReqs(void*) {
    // Request all context fields
    return (1 << 1) | (1 << 2) | (1 << 3) | (1 << 4) | (1 << 5) |
           (1 << 6) | (1 << 7) | (1 << 8) | (1 << 9) | (1 << 10);
}

static IProcessContextRequirementsVtbl g_pcr_vtbl = {
    pcr_qi, pcr_addRef, pcr_release, pcr_getReqs
};

// --- UnitInfo vtable functions ---
#define UINFO(self) (com_from_unitinfo(self)->impl)
static tresult ui_qi(void* s, const TUID iid, void** obj) { return UINFO(s).queryInterface(com_from_unitinfo(s), iid, obj); }
static uint32 ui_addRef(void* s) { return UINFO(s).addRef(); }
static uint32 ui_release(void* s) { auto* com = com_from_unitinfo(s); auto r = com->impl.release(); if (r == 0) { com->impl.~MooseComponent(); free(com); } return r; }
static int32 ui_getUnitCount(void*) { return g_num_units; }
static tresult ui_getUnitInfo(void*, int32 idx, UnitInfo* out) {
    if (idx < 0 || idx >= g_num_units || !out) return kInvalidArgument;
    out->id = g_units[idx].id;
    out->parentUnitId = g_units[idx].parentId;
    str_to_char16(out->name, g_units[idx].name, 128);
    out->programListId = -1; // kNoProgramListId
    return kResultOk;
}
static int32 ui_getProgramListCount(void*) { return 0; }
// Typed `kNotImplemented` stubs for each of the IUnitInfo vtable
// slots we don't care about. One stub per unique signature keeps
// modern clang's `-Wcast-function-type-mismatch` happy - casting a
// nullary function pointer to specific signatures is UB in strict
// readings and warns on arm64 macOS clang.
static tresult ui_ni_iv(void*, int32, void*)                         { return kNotImplemented; }
static tresult ui_ni_iiw(void*, int32, int32, char16*)               { return kNotImplemented; }
static tresult ui_ni_iisw(void*, int32, int32, const char*, char16*) { return kNotImplemented; }
static tresult ui_ni_ii(void*, int32, int32)                         { return kNotImplemented; }
static tresult ui_ni_iihw(void*, int32, int32, int16_t, char16*)     { return kNotImplemented; }
static tresult ui_ni_iiiii_ip(void*, int32, int32, int32, int32, int32*) { return kNotImplemented; }
static tresult ui_ni_iiiv(void*, int32, int32, void*)                { return kNotImplemented; }

static IUnitInfoVtbl g_unitinfo_vtbl = {
    ui_qi, ui_addRef, ui_release,
    ui_getUnitCount, ui_getUnitInfo, ui_getProgramListCount,
    ui_ni_iv,
    ui_ni_iiw,
    ui_ni_iisw,
    ui_ni_ii,
    ui_ni_iihw,
    [](void*) -> int32 { return 0; },
    [](void*, int32) -> tresult { return kResultOk; },
    ui_ni_iiiii_ip,
    ui_ni_iiiv,
};

// --- NoteExpressionController vtable functions ---
#define NEXP(self) (com_from_note_expression(self)->impl)

// The predefined VST3 note-expression types moose bridges to per-note
// MIDI 2.0 events (PerNoteCC 7/10/1/11/74 and PerNotePitchBend for
// tuning). Order is the enumeration order hosts see. Flags value 1 is
// the SDK's kIsBipolar. Volume's domain is `plain = 20 * log(4 *
// norm)` per the SDK, so its no-change default is unity gain at 0.25,
// not 1.0 (which is +12 dB).
static const struct {
    uint32 typeId;
    const char* title;
    const char* shortTitle;
    double defaultValue;
    int32 flags;
} g_note_expression_types[] = {
    {0, "Volume",     "Vol",  0.25, 0},
    {1, "Pan",        "Pan",  0.5,  1},
    {2, "Tuning",     "Tun",  0.5,  1},
    {3, "Vibrato",    "Vibr", 0.0,  0},
    {4, "Expression", "Expr", 1.0,  0},
    {5, "Brightness", "Bri",  0.5,  0},
};
static const int32 kNumNoteExpressionTypes =
    (int32)(sizeof(g_note_expression_types) / sizeof(g_note_expression_types[0]));

static tresult nexp_qi(void* s, const TUID iid, void** obj) { return NEXP(s).queryInterface(com_from_note_expression(s), iid, obj); }
static uint32 nexp_addRef(void* s) { return NEXP(s).addRef(); }
static uint32 nexp_release(void* s) { auto* com = com_from_note_expression(s); auto r = com->impl.release(); if (r == 0) { com->impl.~MooseComponent(); free(com); } return r; }
static int32 nexp_getCount(void*, int32 busIndex, int16_t /*channel*/) {
    if (!g_desc || busIndex < 0 || busIndex >= g_desc->midi_input_ports) return 0;
    return kNumNoteExpressionTypes;
}
static tresult nexp_getInfo(void* s, int32 busIndex, int16_t channel, int32 index,
                            NoteExpressionTypeInfo* info) {
    if (!info || nexp_getCount(s, busIndex, channel) == 0 ||
        index < 0 || index >= kNumNoteExpressionTypes)
        return kInvalidArgument;
    const auto& t = g_note_expression_types[index];
    memset(info, 0, sizeof(*info));
    info->typeId = t.typeId;
    str_to_char16(info->title, t.title, 128);
    str_to_char16(info->shortTitle, t.shortTitle, 128);
    info->unitId = -1; // kNoUnitId: not tied to an IUnitInfo unit
    info->valueDesc.defaultValue = t.defaultValue;
    info->valueDesc.minimum = 0.0;
    info->valueDesc.maximum = 1.0;
    info->valueDesc.stepCount = 0; // continuous
    info->flags = t.flags;
    return kResultOk;
}
// SDK physical domains: tuning's normalized 0..1 spans -120..+120
// semitones (0.5 = no detune), volume is `plain = 20 * log(4 * norm)`
// dB; the other types read naturally as percent.
static tresult nexp_getStringByValue(void*, int32 /*busIndex*/, int16_t /*channel*/,
                                     uint32 typeId, double value, char16* string) {
    if (!string) return kInvalidArgument;
    char buf[64];
    if (typeId == 2) snprintf(buf, sizeof(buf), "%.2f st", (value - 0.5) * 240.0);
    else if (typeId == 0 && value <= 0.0) snprintf(buf, sizeof(buf), "-inf dB");
    else if (typeId == 0) snprintf(buf, sizeof(buf), "%.1f dB", 20.0 * log10(4.0 * value));
    else snprintf(buf, sizeof(buf), "%.1f %%", value * 100.0);
    str_to_char16(string, buf, 128);
    return kResultOk;
}
static tresult nexp_getValueByString(void*, int32 /*busIndex*/, int16_t /*channel*/,
                                     uint32 typeId, const char16* string, double* value) {
    if (!string || !value) return kResultFalse;
    char buf[64];
    int i = 0;
    for (; string[i] && i < 63; i++) buf[i] = (char)string[i];
    buf[i] = 0;
    char* end = nullptr;
    double parsed = strtod(buf, &end);
    if (end == buf) return kResultFalse;
    double v;
    if (typeId == 2) v = parsed / 240.0 + 0.5;
    else if (typeId == 0) v = pow(10.0, parsed / 20.0) / 4.0;
    else v = parsed / 100.0;
    if (v < 0.0) v = 0.0;
    if (v > 1.0) v = 1.0;
    *value = v;
    return kResultOk;
}

static INoteExpressionControllerVtbl g_nexp_vtbl = {
    nexp_qi, nexp_addRef, nexp_release,
    nexp_getCount, nexp_getInfo, nexp_getStringByValue, nexp_getValueByString
};

static MooseComponentCOM* create_component() {
    auto* com = (MooseComponentCOM*)calloc(1, sizeof(MooseComponentCOM));
    if (!com) return nullptr;
    com->vtbl_component = &g_comp_vtbl;
    com->vtbl_processor = &g_proc_vtbl;
    com->vtbl_controller = &g_ctrl_vtbl;
    com->vtbl_host_editing = &g_hedit_vtbl;
    com->vtbl_midimapping = &g_mmap_vtbl;
    com->vtbl_pcr = &g_pcr_vtbl;
    com->vtbl_unitinfo = &g_unitinfo_vtbl;
    com->vtbl_note_expression = &g_nexp_vtbl;
    new (&com->impl) MooseComponent();
    return com;
}

// ---------------------------------------------------------------------------
// Factory
// ---------------------------------------------------------------------------

struct FactoryCOM {
    void* vtbl;
    std::atomic<int32> refCount{1};
};

// IPluginFactory2 and IPluginFactory3 IIDs
static const TUID IPluginFactory2_iid = MAKE_IID(0x0007B650, 0xF24B4C0B, 0xA464EDB9, 0xF00B2ABB);
static const TUID IPluginFactory3_iid = MAKE_IID(0x4555A2AB, 0xC1234E57, 0x9B122910, 0x36878931);

static tresult factory_qi(void* self, const TUID iid, void** obj) {
    if (iid_equal(iid, FUnknown_iid) || iid_equal(iid, IPluginFactory_iid) ||
        iid_equal(iid, IPluginFactory2_iid) || iid_equal(iid, IPluginFactory3_iid)) {
        auto* f = (FactoryCOM*)self;
        f->refCount++;
        *obj = self;
        return kResultOk;
    }
    *obj = nullptr;
    return kNoInterface;
}

static uint32 factory_addRef(void* self) {
    return ++(((FactoryCOM*)self)->refCount);
}

static uint32 factory_release(void* self) {
    auto r = --(((FactoryCOM*)self)->refCount);
    // Don't free - factory is a global static
    return r;
}

static tresult factory_getInfo(void*, PFactoryInfo* info) {
    if (!info || !g_desc) return kInvalidArgument;
    memset(info, 0, sizeof(*info));
    str_to_char8(info->vendor, g_desc->vendor, 64);
    str_to_char8(info->url, g_desc->url, 256);
    str_to_char8(info->email, g_desc->email ? g_desc->email : "", 128);
    info->flags = (1 << 4); // kUnicode
    return kResultOk;
}

static int32 factory_countClasses(void*) { return 1; }

static tresult factory_getClassInfo(void*, int32 index, PClassInfo* info) {
    if (index != 0 || !info || !g_desc) return kInvalidArgument;
    memcpy(info->cid, g_desc->cid, 16);
    info->cardinality = 0x7FFFFFFF; // kManyInstances
    str_to_char8(info->category, g_desc->category, 32);
    str_to_char8(info->name, g_desc->name, 64);
    return kResultOk;
}

// IPluginFactory2::getClassInfo2
struct PClassInfo2 {
    TUID cid; int32 cardinality; char8 category[32]; char8 name[64];
    uint32 classFlags; char8 subCategories[128]; char8 vendor[64]; char8 version[64]; char8 sdkVersion[64];
};

static tresult factory_getClassInfo2(void*, int32 index, PClassInfo2* info) {
    if (index != 0 || !info || !g_desc) return kInvalidArgument;
    memset(info, 0, sizeof(*info));
    memcpy(info->cid, g_desc->cid, 16);
    info->cardinality = 0x7FFFFFFF;
    str_to_char8(info->category, g_desc->category, 32);
    str_to_char8(info->name, g_desc->name, 64);
    info->classFlags = 0; // single-component (processor + controller in one object)
    str_to_char8(info->subCategories, g_desc->subcategories, 128);
    str_to_char8(info->vendor, g_desc->vendor, 64);
    str_to_char8(info->version, g_desc->version, 64);
    str_to_char8(info->sdkVersion, "VST 3.7.1", 64);
    return kResultOk;
}

// IPluginFactory3::getClassInfoUnicode
struct PClassInfoW {
    TUID cid; int32 cardinality; char8 category[32]; char16 name[64];
    uint32 classFlags; char8 subCategories[128]; char16 vendor[64]; char16 version[64]; char16 sdkVersion[64];
};

static tresult factory_getClassInfoW(void*, int32 index, PClassInfoW* info) {
    if (index != 0 || !info || !g_desc) return kInvalidArgument;
    memset(info, 0, sizeof(*info));
    memcpy(info->cid, g_desc->cid, 16);
    info->cardinality = 0x7FFFFFFF;
    str_to_char8(info->category, g_desc->category, 32);
    str_to_char16(info->name, g_desc->name, 64);
    info->classFlags = 0; // single-component
    str_to_char8(info->subCategories, g_desc->subcategories, 128);
    str_to_char16(info->vendor, g_desc->vendor, 64);
    str_to_char16(info->version, g_desc->version, 64);
    str_to_char16(info->sdkVersion, "VST 3.7.1", 64);
    return kResultOk;
}

static tresult factory_setHostContext(void*, void*) { return kResultOk; }

static tresult factory_createInstance(void*, FIDString cid, FIDString iid, void** obj) {
    if (!cid || !iid || !obj || !g_desc) return kInvalidArgument;
    if (memcmp(cid, g_desc->cid, 16) != 0) return kResultFalse;

    auto* com = create_component();
    if (!com) return kResultFalse;

    // The plugin constructor (cb_create) returns null when author create() /
    // init() panicked. Fail instantiation so the host surfaces an error
    // instead of silently loading an inert zombie plugin.
    if (!com->impl.constructed()) {
        com->impl.~MooseComponent();
        free(com);
        return kResultFalse;
    }

    tresult r = com->impl.queryInterface(com, reinterpret_cast<const int8_t*>(iid), obj);
    if (r != kResultOk) {
        com->impl.~MooseComponent();
        free(com);
    } else {
        // QI added a ref, release the initial one
        com->impl.release();
    }
    return r;
}

// Combined vtable: IPluginFactory + IPluginFactory2 + IPluginFactory3
// Laid out as C++ single-inheritance: base methods first, derived appended
struct IPluginFactoryVtbl {
    // FUnknown
    tresult (*queryInterface)(void*, const TUID, void**);
    uint32  (*addRef)(void*);
    uint32  (*release)(void*);
    // IPluginFactory
    tresult (*getFactoryInfo)(void*, PFactoryInfo*);
    int32   (*countClasses)(void*);
    tresult (*getClassInfo)(void*, int32, PClassInfo*);
    tresult (*createInstance)(void*, FIDString, FIDString, void**);
    // IPluginFactory2
    tresult (*getClassInfo2)(void*, int32, PClassInfo2*);
    // IPluginFactory3
    tresult (*getClassInfoUnicode)(void*, int32, PClassInfoW*);
    tresult (*setHostContext)(void*, void*);
};

static IPluginFactoryVtbl g_factory_vtbl = {
    factory_qi, factory_addRef, factory_release,
    factory_getInfo, factory_countClasses, factory_getClassInfo, factory_createInstance,
    factory_getClassInfo2,
    factory_getClassInfoW, factory_setHostContext
};

static FactoryCOM g_factory = { &g_factory_vtbl, {1} };

// ---------------------------------------------------------------------------
// Exported entry points
// ---------------------------------------------------------------------------

extern "C" {

void moose_vst3_register(
    const Vst3PluginDescriptor* descriptor,
    const Vst3Callbacks* callbacks,
    const Vst3ParamDescriptor* params,
    uint32_t num_params
) {
    g_desc = descriptor;
    g_cb = callbacks;
    g_params = params;
    g_num_params = num_params;
    // Each MooseComponent seeds its own cur_in/cur_out to this default
    // (first) layout at construction; setBusArrangements moves that
    // instance to any other declared layout.
    build_unit_map();
}

void* moose_vst3_get_factory() {
    g_factory.refCount = 1;
    return &g_factory;
}

// --- IComponentHandler host-notification callbacks ---

static MooseComponent* ctx_lookup(void* ctx) {
    for (int i = 0; i < kMaxInstances; i++) {
        if (g_ctx_map_key[i] == ctx) return g_ctx_map_comp[i];
    }
    return nullptr;
}

void moose_vst3_begin_edit(void* ctx, uint32_t id) {
    auto* comp = ctx_lookup(ctx);
    if (!comp || !comp->componentHandler) return;
    auto beginEdit = (tresult (*)(void*, uint32))(*(void***)comp->componentHandler)[3];
    beginEdit(comp->componentHandler, id);
}

void moose_vst3_perform_edit(void* ctx, uint32_t id, double normalized) {
    auto* comp = ctx_lookup(ctx);
    if (!comp || !comp->componentHandler) return;
    comp->inPerformEdit = true;
    auto performEdit = (tresult (*)(void*, uint32, double))(*(void***)comp->componentHandler)[4];
    performEdit(comp->componentHandler, id, normalized);
    comp->inPerformEdit = false;
    // Editor edits run on the UI thread - drain any latency restart the
    // audio thread flagged for a latency-affecting parameter here too.
    comp->flushPendingRestart();
}

void moose_vst3_end_edit(void* ctx, uint32_t id) {
    auto* comp = ctx_lookup(ctx);
    if (!comp || !comp->componentHandler) return;
    auto endEdit = (tresult (*)(void*, uint32))(*(void***)comp->componentHandler)[5];
    endEdit(comp->componentHandler, id);
    comp->flushPendingRestart();
}

// Plugin -> host restart request. The audio thread calls this to flag a
// pending IComponentHandler::restartComponent (e.g. kLatencyChanged = 8)
// without touching the host: it only sets bits on an atomic. The bits
// are drained by flushPendingRestart() on the next host main-thread
// callback, so restartComponent never fires from the render thread or a
// moose-owned thread. `flags` is a RestartFlags bitmask.
void moose_vst3_mark_restart(void* ctx, int32_t flags) {
    auto* comp = ctx_lookup(ctx);
    if (!comp) return;
    comp->pendingRestart.fetch_or(flags, std::memory_order_release);
#if __APPLE__
    // Wake the main-queue source so the flush happens promptly without
    // waiting on a host param read or edit gesture. merge_data is
    // lock-free and alloc-free, so this stays RT-safe on the audio thread.
    if (comp->restartSource) dispatch_source_merge_data(comp->restartSource, 1);
#endif
    // Linux and Windows do nothing more here: the audio thread only sets
    // the atomic bit above (no syscall, unquestionably RT-safe). The UI
    // thread polls the bit within kRestartTimerIntervalMs - on Linux via
    // the host run-loop timer while an editor is open (with no editor the
    // param-poll / edit-gesture flush drains it), on Windows via the
    // thread timer for the component's whole life.
}

#if __APPLE__
// Main-queue handler for the restart dispatch source (declared up top).
// Resolves the component from its stable ctx key so a torn-down instance
// is a safe no-op, then drains any pending restartComponent.
static void moose_vst3_restart_source_event(void* key) {
    if (MooseComponent* comp = ctx_lookup(key)) comp->flushPendingRestart();
}
#endif

// Plugin -> host resize request. Walk ctx -> MooseComponent ->
// MoosePlugView -> IPlugFrame*, then call IPlugFrame::resizeView.
//
// IPlugFrame vtable layout (FUnknown + one virtual):
//   [0] queryInterface
//   [1] addRef
//   [2] release
//   [3] resizeView(IPlugView*, ViewRect*)
//
// Encoded as the index constant so a future Steinberg extension to
// FUnknown can be caught by a one-line change here rather than a
// magic `[3]` hidden in vtable arithmetic.
static constexpr int32_t kPlugFrameResizeViewIndex = 3;

int32_t moose_vst3_request_resize(void* ctx, uint32_t w, uint32_t h) {
    auto* comp = ctx_lookup(ctx);
    if (!comp || !comp->plugView) return 0;
    auto* pv = comp->plugView;
    if (!pv->frame) return 0;
    ViewRect rect;
    rect.left = 0;
    rect.top = 0;
    rect.right = (int32)w;
    rect.bottom = (int32)h;
    using ResizeViewFn = tresult (*)(void*, void*, ViewRect*);
    auto resizeView = (ResizeViewFn)(*(void***)pv->frame)[kPlugFrameResizeViewIndex];
    tresult result = resizeView(pv->frame, pv, &rect);
    return result == kResultOk ? 1 : 0;
}

} // extern "C"
