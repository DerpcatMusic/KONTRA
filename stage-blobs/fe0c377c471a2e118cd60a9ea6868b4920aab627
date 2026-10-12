// Empty exported CLAP process/parameter-flush cost; no GUI or library data.
// g++ -O2 -std=c++17 -I SDK/include cpu-audit-clap.cpp -ldl -o HOST
#include <clap/clap.h>
#include <dlfcn.h>
#include <algorithm>
#include <array>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <vector>
static void check(bool ok){if(!ok)std::abort();}
struct Events{
    std::vector<clap_event_param_value_t> events;
    clap_input_events_t in{this,[](const clap_input_events_t*e)->uint32_t{return static_cast<Events*>(e->ctx)->events.size();},[](const clap_input_events_t*e,uint32_t i)->const clap_event_header_t*{return &static_cast<Events*>(e->ctx)->events.at(i).header;}};
    clap_output_events_t out{this,[](const clap_output_events_t*,const clap_event_header_t*){return true;}};
};
int main(int argc,char**argv){
    check(argc==2);void* module=dlopen(argv[1],RTLD_NOW|RTLD_LOCAL);check(module);
    auto* entry=static_cast<const clap_plugin_entry_t*>(dlsym(module,"clap_entry"));check(entry&&entry->init(argv[1]));
    auto* factory=static_cast<const clap_plugin_factory_t*>(entry->get_factory(CLAP_PLUGIN_FACTORY_ID));check(factory);
    auto* desc=factory->get_plugin_descriptor(factory,0);check(desc);
    clap_host_t host{CLAP_VERSION,nullptr,"CPU audit","KONTRA","","1",[](const clap_host_t*,const char*)->const void*{return nullptr;},[](const clap_host_t*){},[](const clap_host_t*){},[](const clap_host_t*){}};
    auto* p=factory->create_plugin(factory,&host,desc->id);check(p&&p->init(p));
    auto* ports=static_cast<const clap_plugin_audio_ports_t*>(p->get_extension(p,CLAP_EXT_AUDIO_PORTS));check(ports);
    auto* params=static_cast<const clap_plugin_params_t*>(p->get_extension(p,CLAP_EXT_PARAMS));check(params&&params->count(p)>0);
    clap_param_info_t param{};check(params->get_info(p,0,&param));
    const auto n=ports->count(p,false);
    std::vector<std::vector<std::array<float,256>>> pcm(n);
    std::vector<std::vector<float*>> ptrs(n);std::vector<clap_audio_buffer_t> outputs(n);
    for(unsigned i=0;i<n;i++){clap_audio_port_info_t info{};check(ports->get(p,i,false,&info));pcm[i].resize(info.channel_count);for(auto&c:pcm[i])ptrs[i].push_back(c.data());outputs[i]={ptrs[i].data(),nullptr,info.channel_count,0,0};}
    Events events;clap_process_t process{};process.audio_outputs=outputs.data();process.audio_outputs_count=n;process.in_events=&events.in;process.out_events=&events.out;
    check(p->activate(p,48000.,32,256)&&p->start_processing(p));
    for(unsigned block:{32,64,256})for(unsigned count:{0,1,64})for(bool flush:{false,true}){
        process.frames_count=block;events.events.resize(count);
        for(auto&e:events.events){e={};e.header={sizeof(e),0,CLAP_CORE_EVENT_SPACE_ID,CLAP_EVENT_PARAM_VALUE,0};e.param_id=param.id;e.note_id=e.port_index=e.channel=e.key=-1;e.value=param.default_value;}
        auto call=[&]{if(flush)params->flush(p,&events.in,&events.out);else{check(p->process(p,&process)!=CLAP_PROCESS_ERROR);process.steady_time+=block;}};
        for(int i=0;i<1000;i++)call();
        std::vector<double> times;times.reserve(10000);
        for(int i=0;i<10000;i++){auto t=std::chrono::steady_clock::now();call();times.push_back(std::chrono::duration<double,std::micro>(std::chrono::steady_clock::now()-t).count());}
        std::sort(times.begin(),times.end());
        std::printf("{\"version\":\"%s\",\"block\":%u,\"events\":%u,\"flush\":%s,\"p50_us\":%.3f,\"p99_us\":%.3f,\"max_us\":%.3f}\n",desc->version,block,count,flush?"true":"false",times[5000],times[9900],times.back());
    }
    p->stop_processing(p);p->deactivate(p);p->destroy(p);entry->deinit();dlclose(module);
}
