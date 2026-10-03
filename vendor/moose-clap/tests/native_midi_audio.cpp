// Real exported CLAP plugin: official SDK headers, original tone, no editor.
// g++ -std=c++17 -Wall -Wextra -Werror -pedantic -O2 -I SDK/include THIS -ldl -pthread -o HOST
// HOST /absolute/libkontakto.so omni.state home7.state lower-zone-home7.state
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

static void require(bool ok, const char* message) {
    if (!ok) { std::fprintf(stderr, "FAIL: %s\n", message); std::exit(1); }
}
struct Stream {
    std::vector<char> bytes;
    size_t cursor = 0;
    clap_istream_t api{this, [](const clap_istream_t* stream, void* out, uint64_t size)->int64_t {
        auto& s = *static_cast<Stream*>(stream->ctx);
        const size_t n = std::min<uint64_t>(size, s.bytes.size()-s.cursor);
        std::memcpy(out, s.bytes.data()+s.cursor, n); s.cursor += n; return n;
    }};
};
union Input { clap_event_header_t header; clap_event_note_t note; clap_event_note_expression_t expression; clap_event_midi_t midi; };
struct Events {
    std::array<Input,24> inputs{};
    uint32_t count = 0;
    std::array<clap_event_note_t,128> ends{};
    uint32_t end_count = 0;
    clap_input_events_t in{this,
        [](const clap_input_events_t* e)->uint32_t { return static_cast<Events*>(e->ctx)->count; },
        [](const clap_input_events_t* e, uint32_t n)->const clap_event_header_t* {
            const auto& events = *static_cast<Events*>(e->ctx);
            return n<events.count ? &events.inputs[n].header : nullptr;
        }};
    clap_output_events_t out{this, [](const clap_output_events_t* e, const clap_event_header_t* h)->bool {
        auto& events = *static_cast<Events*>(e->ctx);
        if (h->space_id==CLAP_CORE_EVENT_SPACE_ID && h->type==CLAP_EVENT_NOTE_END) {
            if (h->size<sizeof(clap_event_note_t) || events.end_count==events.ends.size()) return false;
            events.ends[events.end_count++] = *reinterpret_cast<const clap_event_note_t*>(h);
        }
        return true;
    }};
    Input& add(uint16_t type, uint32_t size, uint32_t time) {
        require(count<inputs.size(), "bounded host event list");
        require(count==0 || inputs[count-1].header.time<=time, "SDK time-ordered events");
        auto& e = inputs[count++]; e = {}; e.header={size,time,CLAP_CORE_EVENT_SPACE_ID,type,CLAP_EVENT_IS_LIVE}; return e;
    }
    void note(bool on, int channel, int id, int key=60, uint32_t time=16) {
        auto& e=add(on?CLAP_EVENT_NOTE_ON:CLAP_EVENT_NOTE_OFF,sizeof(clap_event_note_t),time).note;
        e.note_id=id; e.port_index=0; e.channel=channel; e.key=key; e.velocity=on?0.8:0.;
    }
    void tuning(int channel, int id, double semitones, uint32_t time=16) {
        auto& e=add(CLAP_EVENT_NOTE_EXPRESSION,sizeof(clap_event_note_expression_t),time).expression;
        e.expression_id=CLAP_NOTE_EXPRESSION_TUNING; e.note_id=id; e.port_index=0; e.channel=channel; e.key=60; e.value=semitones;
    }
    void midi(uint8_t status, uint8_t data1, uint8_t data2, uint32_t time=0) {
        auto& e=add(CLAP_EVENT_MIDI,sizeof(clap_event_midi_t),time).midi;
        e.port_index=0; e.data[0]=status; e.data[1]=data1; e.data[2]=data2;
    }
};
struct Host {
    std::thread::id main=std::this_thread::get_id();
    std::atomic<bool> callback{false}, restart{false};
    bool audio=false;
    clap_host_thread_check_t threads{
        [](const clap_host_t* h)->bool { return std::this_thread::get_id()==static_cast<Host*>(h->host_data)->main; },
        [](const clap_host_t* h)->bool { auto& s=*static_cast<Host*>(h->host_data); return std::this_thread::get_id()==s.main && s.audio; }};
    clap_host_t api{CLAP_VERSION,this,"Original tone native host","AuthoredTest","","1",
        [](const clap_host_t* h,const char* id)->const void* {
            auto& s=*static_cast<Host*>(h->host_data);
            return std::strcmp(id,CLAP_EXT_THREAD_CHECK)==0 ? &s.threads : nullptr;
        },
        [](const clap_host_t* h) { static_cast<Host*>(h->host_data)->restart=true; },
        [](const clap_host_t*) {},
        [](const clap_host_t* h) { static_cast<Host*>(h->host_data)->callback=true; }};
};
struct Session {
    Host host;
    Events events;
    const clap_plugin_t* plugin;
    uint32_t dialects=0;
    std::vector<std::vector<std::array<float,128>>> pcm;
    std::vector<std::vector<float*>> pointers;
    std::vector<clap_audio_buffer_t> outputs;
    clap_process_t process{};
    Session(const clap_plugin_factory_t* factory, const char* id, const char* state_path) {
        plugin=factory->create_plugin(factory,&host.api,id);
        require(plugin && plugin->init(plugin),"actual exported CLAP create/init");
        auto* state=static_cast<const clap_plugin_state_t*>(plugin->get_extension(plugin,CLAP_EXT_STATE));
        std::ifstream file(state_path,std::ios::binary);
        Stream stream{{std::istreambuf_iterator<char>(file),std::istreambuf_iterator<char>()}};
        require(state && !stream.bytes.empty() && state->load(plugin,&stream.api),"actual CLAP state load");
        auto* notes=static_cast<const clap_plugin_note_ports_t*>(plugin->get_extension(plugin,CLAP_EXT_NOTE_PORTS));
        clap_note_port_info_t note{};
        require(notes && notes->count(plugin,true)>0 && notes->get(plugin,0,true,&note),"declared note input");
        dialects=note.supported_dialects;
        require((dialects & (CLAP_NOTE_DIALECT_CLAP|CLAP_NOTE_DIALECT_MIDI))==3,"advertised CLAP and MIDI dialects");
        auto* ports=static_cast<const clap_plugin_audio_ports_t*>(plugin->get_extension(plugin,CLAP_EXT_AUDIO_PORTS));
        require(ports && ports->count(plugin,false)>0 && ports->count(plugin,false)<=16,"bounded declared output ports");
        const auto count=ports->count(plugin,false); pcm.resize(count); pointers.resize(count); outputs.resize(count);
        for(uint32_t b=0;b<count;++b) {
            clap_audio_port_info_t info{}; require(ports->get(plugin,b,false,&info),"output port info");
            require(info.channel_count>0 && info.channel_count<=16,"bounded output channel count");
            pcm[b].resize(info.channel_count); for(auto& c:pcm[b]) pointers[b].push_back(c.data());
            outputs[b]={pointers[b].data(),nullptr,info.channel_count,0,0};
        }
        process.audio_outputs=outputs.data(); process.audio_outputs_count=count;
        process.frames_count=128; process.in_events=&events.in; process.out_events=&events.out;
        require(plugin->activate(plugin,48000.,128,128),"activate");
        host.audio=true; require(plugin->start_processing(plugin),"start_processing"); host.audio=false;
        for(int i=0;i<400;++i) { block(); std::this_thread::sleep_for(std::chrono::milliseconds(1)); }
    }
    double block() {
        for(auto& b:pcm) for(auto& c:b) c.fill(0.f);
        host.audio=true; const auto status=plugin->process(plugin,&process); host.audio=false;
        require(status!=CLAP_PROCESS_ERROR,"real CLAP process");
        events.count=0; process.steady_time+=128;
        double energy=0.; for(const auto& b:pcm) for(const auto& c:b) for(float x:c) {
            require(std::isfinite(x),"finite exported PCM"); energy+=x*x;
        }
        if(host.callback.exchange(false)) plugin->on_main_thread(plugin);
        require(!host.restart,"unexpected host restart request");
        return energy;
    }
    void onset() {
        require(std::all_of(pcm[0][0].begin(),pcm[0][0].begin()+16,[](float x){return x==0.f;}),"sample16 onset prefix silence");
        require(std::any_of(pcm[0][0].begin()+16,pcm[0][0].end(),[](float x){return x!=0.f;}),"sample16 onset must sound in same block");
    }
    double tone(double cents, bool check_onset=true) {
        std::array<float,2176> captured{};
        for(int b=0;b<17;++b) { block(); if(b==0 && check_onset) onset(); std::copy(pcm[0][0].begin(),pcm[0][0].end(),captured.begin()+128*b); }
        double first=0.,last=0.; size_t crossings=0; double energy=0.;
        for(float x:captured) energy+=x*x;
        for(size_t n=513;n<captured.size();++n) if(captured[n-1]<=0.f && captured[n]>0.f) {
            const double at=n-1-captured[n-1]/double(captured[n]-captured[n-1]);
            if(crossings++==0) first=at;
            last=at;
        }
        require(energy>1e-4 && crossings>=4,"audible stable original tone");
        const double hz=(crossings-1)*48000./(last-first);
        std::printf(" frequency_hz=%.6f energy=%.9g\n",hz,energy);
        require(std::abs(hz/(261.625565*std::exp2(cents/1200.))-1.)<0.002,"independent tuning frequency"); return hz;
    }
    void quiet() { double tail=0.; for(int b=0;b<400;++b) tail=block(); require(tail<1e-10,"Off/pedal-up must silence before original 3s sample exhaustion"); }
    ~Session() {
        host.audio=true; plugin->stop_processing(plugin); host.audio=false;
        plugin->deactivate(plugin); plugin->destroy(plugin);
    }
};
int main(int argc,char** argv) {
    require(argc==5,"plugin, Omni, home7 and lower-zone state paths required");
    void* module=dlopen(argv[1],RTLD_NOW|RTLD_LOCAL);
    if(!module) { std::fprintf(stderr,"%s\n",dlerror()); return 1; }
    auto* entry=static_cast<const clap_plugin_entry_t*>(dlsym(module,"clap_entry"));
    require(entry && clap_version_is_compatible(entry->clap_version) && entry->init(argv[1]),"exported clap_entry/init");
    auto* factory=static_cast<const clap_plugin_factory_t*>(entry->get_factory(CLAP_PLUGIN_FACTORY_ID));
    require(factory && factory->get_plugin_count(factory)>0,"actual CLAP factory");
    const auto* descriptor=factory->get_plugin_descriptor(factory,0); require(descriptor,"plugin descriptor");
    {
        Session s(factory,descriptor->id,argv[2]);
        std::printf("advertised_input_dialects=%u\n",s.dialects);
        for(int id:{10,-1}) for(double cents:{0.,250.,-350.}) {
            s.events.note(true,0,id); s.events.tuning(0,id,cents/100.);
            std::printf("clap_note id=%d cents=%.0f",id,cents); s.tone(cents);
            s.events.note(false,0,id,60,64); s.quiet();
        }
        for(int bend:{8192,12288}) {
            s.events.midi(0xe0,bend&127,bend>>7); s.events.midi(0x90,60,102,16);
            std::printf("raw_midi bend=%d unassigned_pitch_destination=true",bend); s.tone(0.);
            s.events.midi(0x80,60,0,64); s.quiet();
        }
        s.events.midi(0xe0,0,64); s.block();
        for(bool raw:{true,false}) for(int pedal:{64,66}) {
            if(pedal==64) { s.events.midi(0xb0,pedal,127); s.block(); }
            if(raw) s.events.midi(0x90,60,102,16); else s.events.note(true,0,30+pedal);
            std::printf("pedal dialect=%s cc=%d",raw?"MIDI":"CLAP",pedal); s.tone(0.);
            double before_off=0.; for(int b=0;b<100;++b) before_off=s.block();
            std::printf("before_off_energy=%.9g\n",before_off); require(before_off>1e-4,"held original sample must still sound before Off");
            if(pedal==66) { s.events.midi(0xb0,pedal,127); s.block(); }
            if(raw) s.events.midi(0x80,60,0,64); else s.events.note(false,0,30+pedal,60,64);
            double held=0.; for(int b=0;b<100;++b) held=s.block();
            std::printf("held_energy=%.9g note_ends=%u\n",held,s.events.end_count);
            require(held>1e-4,"pedal must retain released note");
            s.events.midi(0xb0,pedal,0,32); s.quiet();
            std::printf("pedal_held_energy=%.9g up_silent=true\n",held);
        }
        // A released ID remains a valid expression target, but it must never
        // retune a new same-pitch owner. NOTE_END independently closes each ID.
        const auto first_end=s.events.end_count;
        s.events.note(true,0,400); s.tone(0.);
        s.events.note(true,0,401); s.block();
        s.events.note(false,0,400,60,64); s.events.tuning(0,400,12.,64); s.block();
        for(int b=0;b<400;++b) s.block();
        std::printf("exact_overlap_old_expression_new_id"); s.tone(0.,false);
        auto ended=[&](int id) { return std::any_of(s.events.ends.begin()+first_end,s.events.ends.begin()+s.events.end_count,
            [&](const auto& e){return e.note_id==id && e.channel==0 && e.key==60 && e.port_index==0;}); };
        require(ended(400) && !ended(401),"NOTE_END must close only released owner");
        s.events.note(false,0,401,60,64); s.quiet(); require(ended(401),"final exact owner NOTE_END");
        // Sostenuto captures notes already down, not subsequent notes.
        s.events.midi(0xb0,66,127); s.block(); s.events.note(true,0,200); s.tone(0.);
        s.events.note(false,0,200,60,64); s.quiet(); s.events.midi(0xb0,66,0); s.block();
        std::puts("sostenuto_before_note_not_captured=true");
    }
    {
        Session s(factory,descriptor->id,argv[3]);
        s.events.note(true,0,300); double wrong=0.; for(int b=0;b<17;++b) wrong+=s.block();
        require(wrong==0.,"home7 must reject unrelated native channel0");
        s.events.note(false,0,300); s.block(); s.events.note(true,7,301);
        std::printf("home7_clap"); s.tone(0.); s.events.note(false,7,301,60,64); s.quiet();
        s.events.midi(0x90,60,102,16); wrong=0.; for(int b=0;b<17;++b) wrong+=s.block();
        require(wrong==0.,"home7 must reject unrelated raw MIDI channel0"); s.events.midi(0x80,60,0); s.block();
        s.events.midi(0x97,60,102,16); std::printf("home7_midi"); s.tone(0.); s.events.midi(0x87,60,0,64); s.quiet();
    }
    {
        Session s(factory,descriptor->id,argv[4]);
        if(!(s.dialects&CLAP_NOTE_DIALECT_MIDI_MPE)) std::puts("SKIP advertised_MPE: MIDI_MPE dialect not advertised; following checks are configured raw-MIDI zone routing only");
        // Lower zone member1: manual member range48, independent master range2.
        // 1/48 full-scale member bend =>1 semitone; half master bend =>1.
        const int member=8192+171;
        // Explicit manager RPN0 removes dependence on a library PB_PITCH modulator.
        s.events.midi(0xb0,101,0); s.events.midi(0xb0,100,0);
        s.events.midi(0xb0,6,2); s.events.midi(0xb0,38,0);
        s.events.midi(0xb0,101,127); s.events.midi(0xb0,100,127);
        s.events.midi(0xe0,0,96); s.events.midi(0xe1,member&127,member>>7);
        s.events.midi(0x91,60,102,16); std::printf("configured_lower_member1_plus_manager cents=200.1953125"); s.tone(200.1953125);
        s.events.midi(0x81,60,0,64); s.quiet();
        s.events.midi(0x92,60,102,16); std::printf("configured_lower_member2_manager_only cents=100"); s.tone(100.);
        s.events.midi(0x82,60,0,64); s.quiet();
        s.events.midi(0xe0,0,64); s.events.midi(0xe1,0,64); s.block();
        for(int pedal:{64,66}) {
            if(pedal==64) { s.events.midi(0xb0,pedal,127); s.block(); }
            s.events.midi(0x91,60,102,16); std::printf("configured_lower_manager_pedal cc=%d",pedal); s.tone(0.);
            if(pedal==66) { s.events.midi(0xb0,pedal,127); s.block(); }
            s.events.midi(0x81,60,0,64); double held=0.; for(int b=0;b<100;++b) held=s.block();
            require(held>1e-4,"MPE manager pedal must retain released member");
            // The same pedal must not sustain the unrelated part home outside
            // this zone. Stop the member first, then prove that exclusion.
            s.events.midi(0xb0,pedal,0,32); s.quiet();
            s.events.midi(0x97,60,102,16); std::printf("outside_zone_home7 cc=%d",pedal); s.tone(0.);
            s.events.midi(0xb0,pedal,127); s.block(); s.events.midi(0x87,60,0,64); s.quiet();
            s.events.midi(0xb0,pedal,0); s.block();
            std::printf("manager_member_held=%.9g home_off_silent=true\n",held);
        }
    }
    entry->deinit(); dlclose(module); std::puts("PASS exported CLAP note/MIDI/process/pedal/home/zone gates");
}
