// Actual exported plugin host, not substitute Rust callbacks:
// g++ -std=c++17 -O2 vendor/moose-vst3/tests/native_midi_audio.cpp -ldl -pthread -o /tmp/native-midi-audio
// /tmp/native-midi-audio /absolute/libkontakto.so /absolute/authored-tone.state
#include "../shim/vst3_shim.cpp"
#include <dlfcn.h>
#include <fstream>
#include <vector>
#include <array>
#include <thread>
#include <chrono>
#include <cmath>
#include <algorithm>

static void require(bool ok, const char* what) {
    if (!ok) { std::fprintf(stderr, "FAIL: %s\n", what); std::exit(1); }
}
struct Stream {
    IBStreamVtbl* vtbl;
    std::vector<uint8_t> bytes;
    int64_t cursor = 0;
};
static tresult query(void*, const TUID, void** out) { *out=nullptr; return kNoInterface; }
static uint32 retain(void*) { return 1; }
static tresult read_state(void* self, void* out, int32 requested, int32* count) {
    auto& s=*static_cast<Stream*>(self);
    if (requested<0) return kInvalidArgument;
    auto n=std::min<int64_t>(requested,s.bytes.size()-s.cursor);
    std::memcpy(out,s.bytes.data()+s.cursor,n); s.cursor+=n;
    if(count) *count=static_cast<int32>(n);
    return kResultOk;
}
static tresult seek_state(void* self,int64_t offset,int32 mode,int64_t* position) {
    auto& s=*static_cast<Stream*>(self);
    auto next=offset+(mode==1?s.cursor:mode==2?static_cast<int64_t>(s.bytes.size()):0);
    if(next<0 || next>static_cast<int64_t>(s.bytes.size())) return kInvalidArgument;
    s.cursor=next; if(position) *position=next; return kResultOk;
}
static IBStreamVtbl stream_vtbl={query,retain,retain,read_state,
    [](void*,void*,int32,int32*)->tresult{return kNotImplemented;},seek_state,
    [](void* s,int64_t* p)->tresult{*p=static_cast<Stream*>(s)->cursor;return kResultOk;}};
struct EventVtbl {
    tresult (*qi)(void*,const TUID,void**); uint32 (*add)(void*); uint32 (*drop)(void*);
    int32 (*count)(void*); tresult (*get)(void*,int32,void*); tresult (*put)(void*,void*);
};
struct Events { EventVtbl* vtbl; Vst3SdkEvent event{}; bool present=false; };
static EventVtbl event_vtbl={query,retain,retain,
    [](void* s)->int32{return static_cast<Events*>(s)->present?1:0;},
    [](void* s,int32 i,void* out)->tresult{
        auto& e=*static_cast<Events*>(s); if(i!=0 || !e.present)return kInvalidArgument;
        std::memcpy(out,&e.event,sizeof(e.event));return kResultOk;},
    [](void*,void*)->tresult{return kResultOk;}};

int main(int argc,char** argv) {
    require(argc==3,"plugin and authored state paths required");
    void* module=dlopen(argv[1],RTLD_NOW|RTLD_LOCAL);
    if(!module){std::fprintf(stderr,"%s\n",dlerror());return 1;}
    auto entry=reinterpret_cast<bool(*)(void*)>(dlsym(module,"ModuleEntry"));
    require(!entry || entry(module),"ModuleEntry");
    auto get_factory=reinterpret_cast<void*(*)()>(dlsym(module,"GetPluginFactory"));
    require(get_factory!=nullptr,"actual exported GetPluginFactory");
    void* factory=get_factory(); auto* fv=*static_cast<IPluginFactoryVtbl**>(factory);
    PClassInfo info{}; require(fv->getClassInfo(factory,0,&info)==kResultOk,"component class");
    void* component=nullptr;
    require(fv->createInstance(factory,reinterpret_cast<const char*>(info.cid),reinterpret_cast<const char*>(IComponent_iid),&component)==kResultOk,"native component");
    auto* cv=*static_cast<IComponentVtbl**>(component);
    require(cv->initialize(component,nullptr)==kResultOk,"initialize");
    void* processor=nullptr;
    require(cv->queryInterface(component,IAudioProcessor_iid,&processor)==kResultOk,"native processor");
    auto* pv=*static_cast<IAudioProcessorVtbl**>(processor);
    std::ifstream input(argv[2],std::ios::binary);
    Stream state{&stream_vtbl,{std::istreambuf_iterator<char>(input),std::istreambuf_iterator<char>()}};
    require(!state.bytes.empty() && cv->setState(component,&state)==kResultOk,"actual host state load");
    ProcessSetup setup{0,0,128,48000.};
    require(pv->setupProcessing(processor,&setup)==kResultOk,"setupProcessing");
    const auto buses=cv->getBusCount(component,0,1);
    require(buses>0 && buses<=16,"declared output buses");
    std::vector<std::vector<std::array<float,128>>> pcm(buses);
    std::vector<std::vector<float*>> pointers(buses);
    std::vector<AudioBusBuffers> outputs(buses);
    for(int b=0;b<buses;++b){
        BusInfo bus{};require(cv->getBusInfo(component,0,1,b,&bus)==kResultOk,"output bus info");
        pcm[b].resize(bus.channelCount);for(auto& channel:pcm[b]) pointers[b].push_back(channel.data());
        outputs[b]={bus.channelCount,0,{pointers[b].data()}};
        require(cv->activateBus(component,0,1,b,1)==kResultOk,"activate output bus");
    }
    require(cv->activateBus(component,1,0,0,1)==kResultOk,"activate MIDI input bus");
    require(cv->setActive(component,1)==kResultOk,"setActive");
    require(pv->setProcessing(processor,1)==kResultOk,"setProcessing");
    Events events{&event_vtbl};
    ProcessData data{};data.numSamples=128;data.numOutputs=buses;data.outputs=outputs.data();data.inputEvents=&events;
    auto block=[&](){
        for(auto& bus:pcm)for(auto& channel:bus)channel.fill(0.);
        require(pv->process(processor,&data)==kResultOk,"actual native process");
        double energy=0.;for(float x:pcm[0][0]){require(std::isfinite(x),"finite PCM");energy+=x*x;}
        events.present=false;return energy;
    };
    // Give the real loader worker time to install the authored sample bank.
    for(int i=0;i<400;++i){block();std::this_thread::sleep_for(std::chrono::milliseconds(1));}
    auto note=[&](bool on,int id,int length,int offset,float tuning){
        events.event={};events.event.type=on?kVst3NoteOnEvent:kVst3NoteOffEvent;
        events.event.sampleOffset=offset;
        if(on)events.event.noteOn={0,60,tuning,0.8f,length,id};
        else events.event.noteOff={0,60,0.f,id,0.f};
        events.present=true;
    };
    bool passed=true;
    for(bool anonymous:{false,true}) for(int length:{0,16,12000,-1,INT32_MIN}) {
        const int id=anonymous?-1:100+(length==INT32_MIN?99:std::abs(length));
        const float tuning=length==16?250.f:length==12000?-350.f:0.f;
        note(true,id,length,16,tuning);events.event.flags=anonymous?1:0; // SDK Event::kIsLive
        double energy=block();
        std::vector<float> captured(pcm[0][0].begin(),pcm[0][0].end());
        require(std::all_of(pcm[0][0].begin(),pcm[0][0].begin()+16,[](float x){return x==0.f;}),"sample-exact onset");
        for(int i=0;i<16;++i){energy+=block();captured.insert(captured.end(),pcm[0][0].begin(),pcm[0][0].end());}
        double frequency=0.;
        if(energy>1e-4){
            // Positive-going crossings after the attack: independent of the
            // adapter, engine phase and gain, including fractional periods.
            std::vector<double> crossings;
            for(size_t n=513;n<captured.size();++n)if(captured[n-1]<=0.f && captured[n]>0.f)
                crossings.push_back(n-1-captured[n-1]/double(captured[n]-captured[n-1]));
            require(crossings.size()>=4,"stable authored tone crossings");
            frequency=(crossings.size()-1)*48000./(crossings.back()-crossings.front());
            const double expected=261.625565*std::exp2(tuning/1200.);
            require(std::abs(frequency/expected-1.)<0.002,"native cents tuning must match independent tone frequency");
        }
        std::printf("native anonymous=%d length=%d tuning_cents=%.0f attack_energy=%.9g frequency_hz=%.6f\n",anonymous,length,tuning,energy,frequency);
        passed &= energy>1e-4;
        note(false,id,0,64,0.f);block();double tail=0.;
        for(int i=0;i<400;++i)tail=block();
        require(tail<1e-10,"native NoteOff closes sounding owner before sample exhaustion");
    }
    require(pv->setProcessing(processor,0)==kResultOk,"stop processing");
    require(cv->setActive(component,0)==kResultOk,"deactivate");
    cv->terminate(component);pv->release(processor);cv->release(component);fv->release(factory);
    auto exit=reinterpret_cast<bool(*)()>(dlsym(module,"ModuleExit"));if(exit)exit();dlclose(module);
    require(passed,"optional length metadata silenced a valid native host note");
}
