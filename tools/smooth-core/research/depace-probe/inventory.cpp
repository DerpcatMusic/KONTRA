// Ask the unpaced plug-in, in its own words, what it is.
//
// This is a minimal VST3 client: it loads the module, calls the two entry
// points every VST3 exposes, and walks the factory and the edit controller.
// No audio, no host -- the point is that the restored image executes its own
// code far enough to answer, and that the answers (class names, parameter
// names, ranges, defaults) can be compared with SMOOTH's own parameter table.
//
// Build: see run.sh in this directory.

#include <windows.h>

#include <cstdio>

#include "pluginterfaces/base/fplatform.h"
#include "pluginterfaces/base/ipluginbase.h"
#include "pluginterfaces/vst/ivstcomponent.h"
#include "pluginterfaces/vst/ivsteditcontroller.h"
#include "pluginterfaces/vst/ivstaudioprocessor.h"

using namespace Steinberg;
using namespace Steinberg::Vst;

static void print_cid(const TUID& cid) {
    for (int i = 0; i < 16; i++) {
        printf("%02x", static_cast<unsigned char>(cid[i]));
    }
}

static void narrow(const char16* in, char* out, int size) {
    int n = 0;
    for (; n < size - 1 && in[n]; n++) {
        out[n] = in[n] < 128 ? static_cast<char>(in[n]) : '?';
    }
    out[n] = 0;
}

static void walk(IPluginFactory* factory) {
    PFactoryInfo info;
    factory->getFactoryInfo(&info);
    printf("factory vendor=%s url=%s email=%s flags=%d\n", info.vendor, info.url, info.email,
           info.flags);

    int classes = factory->countClasses();
    printf("classes=%d\n", classes);

    {
        auto vtable = *reinterpret_cast<void***>(factory);
        auto base = reinterpret_cast<unsigned char*>(GetModuleHandleW(nullptr));
        (void)base;
        HMODULE self = nullptr;
        GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS |
                               GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                           reinterpret_cast<LPCWSTR>(vtable[0]), &self);
        printf("factory vtable=%p\n", static_cast<void*>(vtable));
        for (int slot = 0; slot < 10; slot++) {
            printf("  slot[%d]=%p\n", slot, vtable[slot]);
        }
    }
    IPluginFactory3* factory3 = nullptr;
    factory->queryInterface(IPluginFactory3::iid, reinterpret_cast<void**>(&factory3));

    TUID component = {};
    TUID controller = {};
    bool haveComponent = false;
    bool haveController = false;

    for (int i = 0; i < classes; i++) {
        {
            PClassInfo classic;
            memset(&classic, 0, sizeof classic);
            tresult cr = factory->getClassInfo(i, &classic);
            printf("classic[%d] result=0x%x name=\"%s\" category=\"%s\" cardinality=%d cid=",
                   i, cr, classic.name, classic.category, classic.cardinality);
            print_cid(classic.cid);
            printf("\n");
        }
        char name[256] = {}, category[64] = {}, sub[256] = {}, vendor[128] = {};
        char version[64] = {}, sdk[64] = {};
        if (factory3) {
            PClassInfoW w;
            memset(&w, 0, sizeof w);
            tresult cr = factory3->getClassInfoUnicode(i, &w);
            if (cr != kResultOk) {
                printf("class[%d] getClassInfoUnicode=0x%x\n", i, cr);
                continue;
            }
            {
                const unsigned char* raw = reinterpret_cast<const unsigned char*>(&w);
                printf("class[%d] raw:", i);
                for (int b = 0; b < 96 && b < static_cast<int>(sizeof w); b++) printf(" %02x", raw[b]);
                printf("\n");
            }
            narrow(w.name, name, sizeof name);
            snprintf(category, sizeof category, "%s", w.category);
            snprintf(sub, sizeof sub, "%s", w.subCategories);
            narrow(w.vendor, vendor, sizeof vendor);
            narrow(w.version, version, sizeof version);
            narrow(w.sdkVersion, sdk, sizeof sdk);
            printf("class[%d] name=\"%s\" category=\"%s\" sub=\"%s\" vendor=\"%s\" "
                   "version=\"%s\" sdk=\"%s\" cardinality=%d flags=0x%x cid=",
                   i, name, category, sub, vendor, version, sdk, w.cardinality, w.classFlags);
            print_cid(w.cid);
            printf("\n");
            if (strcmp(category, kVstAudioEffectClass) == 0 && !haveComponent) {
                memcpy(component, w.cid, sizeof(TUID));
                haveComponent = true;
            }
            if (strcmp(category, kVstComponentControllerClass) == 0 && !haveController) {
                memcpy(controller, w.cid, sizeof(TUID));
                haveController = true;
            }
        } else {
            PClassInfo cls;
            if (factory->getClassInfo(i, &cls) != kResultOk) {
                continue;
            }
            printf("class[%d] name=\"%s\" category=\"%s\" cid=", i, cls.name, cls.category);
            print_cid(cls.cid);
            printf("\n");
        }
    }
    {
        struct Table { long long count; void* records; } table = {-1, nullptr};
        using getter_t = void* (*)(void*);
        auto getter = reinterpret_cast<getter_t>(
            reinterpret_cast<unsigned char*>(GetModuleHandleW(L"soothe3-native.vst3")) + 0x326f40);
        getter(&table);
        printf("class table count=%lld ptr=%p\n", table.count, table.records);
        if (table.records) {
            const unsigned char* rec = static_cast<const unsigned char*>(table.records);
            printf("record[0] head:");
            for (int b = 0; b < 64; b++) printf(" %02x", rec[b]);
            printf("\n");
        }
    }
    printf("classes walked\n");
    if (factory3) {
        factory3->release();
    }
    printf("factory3 released\n");

    if (!haveController) {
        printf("no edit controller class\n");
        return;
    }

    IEditController* edit = nullptr;
    tresult made = factory->createInstance(controller, IEditController::iid,
                                           reinterpret_cast<void**>(&edit));
    printf("createInstance(IEditController)=0x%x\n", made);
    if (made != kResultOk || !edit) {
        return;
    }
    tresult ready = edit->initialize(nullptr);
    printf("initialize()=0x%x\n", ready);

    int count = edit->getParameterCount();
    printf("parameters=%d\n", count);
    for (int i = 0; i < count; i++) {
        ParameterInfo param;
        if (edit->getParameterInfo(i, param) != kResultOk) {
            continue;
        }
        double normalised = edit->getParamNormalized(param.id);
        printf("param[%d] id=%u title=\"%s\" short=\"%s\" unit=\"%s\" stepCount=%d "
               "flags=0x%x default=%.6f value=%.6f\n",
               i, param.id, param.title, param.shortTitle, param.units, param.stepCount,
               param.flags, param.defaultNormalizedValue, normalised);
    }
    edit->terminate();
    edit->release();

    // The processor, not the controller, is where the DSP lives.
    if (haveComponent) {
        IComponent* comp = nullptr;
        printf("creating component...\n");
        tresult made = factory->createInstance(component, IComponent::iid,
                                               reinterpret_cast<void**>(&comp));
        printf("createInstance(IComponent)=0x%x\n", made);
        if (made == kResultOk && comp) {
            tresult ready = comp->initialize(nullptr);
            printf("component->initialize()=0x%x\n", ready);
            printf("component buses: audio in=%d out=%d, event in=%d out=%d\n",
                   comp->getBusCount(kAudio, kInput), comp->getBusCount(kAudio, kOutput),
                   comp->getBusCount(kEvent, kInput), comp->getBusCount(kEvent, kOutput));
            IAudioProcessor* processor = nullptr;
            if (comp->queryInterface(IAudioProcessor::iid,
                                     reinterpret_cast<void**>(&processor)) == kResultOk &&
                processor) {
                printf("processor latencySamples=%d tailSamples=%d\n",
                       processor->getLatencySamples(), processor->getTailSamples());
                processor->release();
            }
            comp->terminate();
            comp->release();
        }
    }

}

// Dump one section of the *mapped* image, so the bytes the reference actually
// executes can be compared with the file on disk and with the restored image.
static void dump_sections(HMODULE module, const char* stem) {
    auto base = reinterpret_cast<unsigned char*>(module);
    auto dos = reinterpret_cast<IMAGE_DOS_HEADER*>(base);
    auto nt = reinterpret_cast<IMAGE_NT_HEADERS64*>(base + dos->e_lfanew);
    auto section = IMAGE_FIRST_SECTION(nt);
    for (unsigned i = 0; i < nt->FileHeader.NumberOfSections; i++) {
        char name[9] = {};
        memcpy(name, section[i].Name, 8);
        DWORD size = section[i].Misc.VirtualSize;
        if (size == 0 || size > 64u * 1024 * 1024) {
            continue;
        }
        char path[260];
        snprintf(path, sizeof(path), "%s-%s.bin", stem, name);
        FILE* file = fopen(path, "wb");
        if (!file) {
            continue;
        }
        fwrite(base + section[i].VirtualAddress, 1, size, file);
        fclose(file);
        printf("dumped %s rva=0x%lx bytes=%lu\n", path,
               static_cast<unsigned long>(section[i].VirtualAddress),
               static_cast<unsigned long>(size));
    }
}

int main(int argc, char** argv) {
    setvbuf(stdout, nullptr, _IONBF, 0);
    const wchar_t* path = argc > 1 ? L"unused" : L"soothe3.vst3";
    (void)path;
    wchar_t module[MAX_PATH];
    MultiByteToWideChar(CP_UTF8, 0, argc > 1 ? argv[1] : "soothe3.vst3", -1, module, MAX_PATH);

    HMODULE loaded = LoadLibraryW(module);
    if (!loaded) {
        printf("LoadLibrary failed=%lu\n", GetLastError());
        return 2;
    }
    printf("loaded=1\n");

    auto init = reinterpret_cast<bool (*)()>(GetProcAddress(loaded, "InitDll"));
    auto factory_of = reinterpret_cast<IPluginFactory* (*)()>(
        GetProcAddress(loaded, "GetPluginFactory"));
    auto exit = reinterpret_cast<bool (*)()>(GetProcAddress(loaded, "ExitDll"));
    printf("exports init=%d factory=%d exit=%d\n", init != nullptr, factory_of != nullptr,
           exit != nullptr);
    if (!init || !factory_of) {
        return 3;
    }

    // Dump before anything can crash: the loader's TLS callbacks have already
    // run, and the question is what they did to the image.
    dump_sections(loaded, "mapped");
    printf("InitDll=%d\n", init());
    IPluginFactory* factory = factory_of();
    if (!factory) {
        printf("no factory\n");
        return 4;
    }
    walk(factory);
    factory->release();
    dump_sections(loaded, "mapped");
    if (exit) {
        printf("ExitDll=%d\n", exit());
    }
    return 0;
}
