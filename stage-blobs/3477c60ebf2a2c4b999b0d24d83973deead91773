// Actual exported SDK event ingestion and complete stereo PCM oracle.
// Builds with g++ -std=c++17 -O2 ... -ldl -pthread; run via
// tools/test_native_note_expression.py PLUGIN CLI OUTPUT_DIRECTORY.
#define main existing_native_midi_host_main
#include "native_midi_audio.cpp"
#undef main

struct ExpressionEvents { EventVtbl* vtbl; std::vector<Vst3SdkEvent> values; };
static EventVtbl expression_events_vtbl={query,retain,retain,
    [](void* p)->int32{return static_cast<int32>(static_cast<ExpressionEvents*>(p)->values.size());},
    [](void* p,int32 i,void* out)->tresult{
        auto& e=static_cast<ExpressionEvents*>(p)->values;
        if(i<0 || static_cast<size_t>(i)>=e.size()) return kInvalidArgument;
        std::memcpy(out,&e[i],sizeof(Vst3SdkEvent));return kResultOk;},
    [](void*,void*)->tresult{return kResultOk;}};

struct Host {
    void* component=nullptr; void* processor=nullptr;
    IComponentVtbl* cv; IAudioProcessorVtbl* pv;
    std::vector<std::vector<std::array<float,128>>> pcm;
    std::vector<std::vector<float*>> pointers;
    std::vector<AudioBusBuffers> outputs;
    ExpressionEvents events{&expression_events_vtbl,{}};
    ProcessData data{};
    Host(void* factory,IPluginFactoryVtbl* fv,const PClassInfo& info,const char* path) {
        require(fv->createInstance(factory,reinterpret_cast<const char*>(info.cid),reinterpret_cast<const char*>(IComponent_iid),&component)==kResultOk,"expression component");
        cv=*static_cast<IComponentVtbl**>(component);
        require(cv->initialize(component,nullptr)==kResultOk,"expression initialize");
        require(cv->queryInterface(component,IAudioProcessor_iid,&processor)==kResultOk,"expression processor");
        pv=*static_cast<IAudioProcessorVtbl**>(processor);
        std::ifstream input(path,std::ios::binary);
        Stream state{&stream_vtbl,{std::istreambuf_iterator<char>(input),std::istreambuf_iterator<char>()}};
        require(!state.bytes.empty() && cv->setState(component,&state)==kResultOk,"authored expression state");
        ProcessSetup setup{0,0,128,48000.};
        require(pv->setupProcessing(processor,&setup)==kResultOk,"expression setup");
        auto buses=cv->getBusCount(component,0,1);require(buses>0 && buses<=16,"expression buses");
        pcm.resize(buses);pointers.resize(buses);outputs.resize(buses);
        for(int b=0;b<buses;++b){
            BusInfo bus{};require(cv->getBusInfo(component,0,1,b,&bus)==kResultOk,"expression bus info");
            pcm[b].resize(bus.channelCount);for(auto& ch:pcm[b])pointers[b].push_back(ch.data());
            outputs[b]={bus.channelCount,0,{pointers[b].data()}};
            require(cv->activateBus(component,0,1,b,1)==kResultOk,"expression activate output");
        }
        require(pcm[0].size()==2,"stereo oracle bus");
        require(cv->activateBus(component,1,0,0,1)==kResultOk,"expression input bus");
        require(cv->setActive(component,1)==kResultOk,"expression active");
        require(pv->setProcessing(processor,1)==kResultOk,"expression processing");
        data.numSamples=128;data.numOutputs=buses;data.outputs=outputs.data();data.inputEvents=&events;
    }
    ~Host(){pv->setProcessing(processor,0);cv->setActive(component,0);cv->terminate(component);pv->release(processor);cv->release(component);}
    void block(){
        for(auto& bus:pcm)for(auto& ch:bus)ch.fill(0.f);
        require(pv->process(processor,&data)==kResultOk,"actual expression process");
        for(auto& ch:pcm[0])for(float x:ch)require(std::isfinite(x),"finite expression PCM");
        events.values.clear();
    }
    void note(bool on,int id,int channel,int offset=0){
        Vst3SdkEvent e{};e.sampleOffset=offset;e.type=on?kVst3NoteOnEvent:kVst3NoteOffEvent;
        if(on)e.noteOn={static_cast<int16_t>(channel),60,0.f,1.f,0,id};
        else e.noteOff={static_cast<int16_t>(channel),60,0.f,id,0.f};
        events.values.push_back(e);
    }
    void expression(int type,int id,double value,int offset=0){
        Vst3SdkEvent e{};e.sampleOffset=offset;e.type=kVst3NoteExpressionValueEvent;
        e.noteExpressionValue={static_cast<uint32_t>(type),id,value};events.values.push_back(e);
    }
    void pressure(int id,int channel,float value,int offset=0){
        Vst3SdkEvent e{};e.sampleOffset=offset;e.type=kVst3PolyPressureEvent;
        e.polyPressure={static_cast<int16_t>(channel),60,value,id};events.values.push_back(e);
    }
};

int main(int argc,char** argv){
    require(argc==5,"plugin, brightness state, pressure state, and literal gain oracle state required");
    void* module=dlopen(argv[1],RTLD_NOW|RTLD_LOCAL);if(!module){std::fprintf(stderr,"%s\n",dlerror());return 1;}
    auto entry=reinterpret_cast<bool(*)(void*)>(dlsym(module,"ModuleEntry"));require(!entry || entry(module),"expression ModuleEntry");
    auto get=reinterpret_cast<void*(*)()>(dlsym(module,"GetPluginFactory"));require(get!=nullptr,"exported expression factory");
    void* factory=get();auto* fv=*static_cast<IPluginFactoryVtbl**>(factory);
    PClassInfo info{};require(fv->getClassInfo(factory,0,&info)==kResultOk,"expression class");
    for(bool pressure:{false,true}){
        Host actual(factory,fv,info,argv[pressure?3:2]), oracle(factory,fv,info,argv[4]);
        for(int b=0;b<400;++b){actual.block();oracle.block();std::this_thread::sleep_for(std::chrono::milliseconds(1));}
        auto value=[&](int id,int channel,double v,int offset=0){
            if(pressure){actual.pressure(id,channel,static_cast<float>(v),offset);v=static_cast<float>(v);}
            else actual.expression(5,id,v,offset);
            // Route-free instrument: SDK volume has independently defined
            // linear gain4*v. It does not consult the CC74/pressure source.
            oracle.expression(0,id,v/4.,offset);
        };
        double error=0.,peak=0.,tail=0.;
        for(int b=0;b<160;++b){
            if(b==0)for(auto pair:{std::pair<int,int>{100,4},{101,7}}){
                for(Host* h:{&actual,&oracle}){h->note(true,pair.first,pair.second,16);h->expression(1,pair.first,pair.second==4?0.:1.,16);}
                value(pair.first,pair.second,1.,16);
            }
            if(b==8){value(100,4,0.5001);value(101,7,0.5030);}
            if(b==24){for(Host* h:{&actual,&oracle}){h->note(false,100,4,32);h->note(true,102,4,32);h->expression(1,102,0.,32);}value(102,4,1.,32);}
            if(b==25)value(100,4,0.25,64); // released old family, never new102
            if(b==40){for(Host* h:{&actual,&oracle})h->note(false,102,4,64);}
            if(b==48){for(Host* h:{&actual,&oracle})h->note(false,101,7,16);}
            if(b==128)value(100,4,0.); // fully retired identity must stay inert
            actual.block();oracle.block();
            for(int c=0;c<2;++c)for(int n=0;n<128;++n){
                error=std::max(error,std::abs(double(actual.pcm[0][c][n])-oracle.pcm[0][c][n]));
                peak=std::max(peak,std::abs(double(oracle.pcm[0][c][n])));
                if(b>=128)tail=std::max(tail,std::abs(double(actual.pcm[0][c][n])));
            }
        }
        require(peak>0.01,"expression oracle must sound");
        require(error<2e-5,"complete native expression PCM differs from literal gain oracle");
        require(tail<1e-8,"exact released families remain silent after retirement");
        std::printf("native %s complete_stereo_frames=20480 max_error=%.9g peak=%.9g terminal_tail=%.9g\n",pressure?"SDK PolyPressure":"Brightness5",error,peak,tail);
    }
    fv->release(factory);auto exit=reinterpret_cast<bool(*)()>(dlsym(module,"ModuleExit"));if(exit)exit();dlclose(module);
}
