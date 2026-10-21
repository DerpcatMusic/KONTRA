// Why does createInstance never return on the depaced image?
//
// The inventory probe showed that class creation hangs. This probe isolates
// the cause: it calls createInstance on its own thread, waits, and if the call
// has not returned it suspends the thread and prints the instruction pointer
// plus every stack word that looks like a return address into the module.
//
// Modes:
//   create_probe.exe <image>            enumerate classes, then create (zero CID)
//   create_probe.exe <image> direct     skip enumeration, create (zero CID)
//   create_probe.exe <image> rand       skip enumeration, create (bogus CID)
//
// If "direct" returns and the default mode hangs, the class enumeration itself
// leaves a lock held. If both hang, creation blocks on its own.

#include <windows.h>

#include <cstdio>
#include <cstring>

#include "pluginterfaces/base/fplatform.h"
#include "pluginterfaces/base/ipluginbase.h"
#include "pluginterfaces/vst/ivstcomponent.h"
#include "pluginterfaces/vst/ivsteditcontroller.h"

using namespace Steinberg;
using namespace Steinberg::Vst;

static HMODULE g_module = nullptr;

// Prints the faulting instruction and every stack word that looks like a return
// address into the module, so the crash can be walked back by hand.
static LONG WINAPI on_exception(EXCEPTION_POINTERS* info) {
    auto base = static_cast<DWORD64>(reinterpret_cast<uintptr_t>(g_module));
    DWORD64 rip = info->ContextRecord->Rip;
    if (info->ExceptionRecord->ExceptionCode == 0x40010006 ||
        info->ExceptionRecord->ExceptionCode == 0xe06d7363) {
        return EXCEPTION_CONTINUE_SEARCH;
    }
    printf("EXCEPTION code=%#lx rip rva=%#llx rsp=%p\n", info->ExceptionRecord->ExceptionCode,
           static_cast<unsigned long long>(rip - base),
           reinterpret_cast<void*>(info->ContextRecord->Rsp));
    if (info->ExceptionRecord->ExceptionCode == EXCEPTION_ACCESS_VIOLATION) {
        printf("   access=%llu address=%p\n",
               static_cast<unsigned long long>(info->ExceptionRecord->ExceptionInformation[0]),
               reinterpret_cast<void*>(info->ExceptionRecord->ExceptionInformation[1]));
    }
    auto* sp = reinterpret_cast<const DWORD64*>(reinterpret_cast<uintptr_t>(info->ContextRecord->Rsp));
    for (int i = 0; i < 128; i++) {
        DWORD64 value = sp[i];
        if (value >= base && value < base + 0x9000000ULL) {
            printf("   stack[%3d] rva=%#llx\n", i, static_cast<unsigned long long>(value - base));
        }
    }
    return EXCEPTION_CONTINUE_SEARCH;
}

struct Job {
    IPluginFactory* factory;
    TUID cid;
    int which;  // 0 component, 1 controller
    tresult result;
    void* object;
    volatile LONG done;
};

static DWORD WINAPI worker(LPVOID param) {
    Job* job = static_cast<Job*>(param);
    void* object = nullptr;
    if (job->which == 0) {
        job->result = job->factory->createInstance(job->cid, IComponent::iid, &object);
    } else {
        job->result = job->factory->createInstance(job->cid, IEditController::iid, &object);
    }
    job->object = object;
    InterlockedExchange(&job->done, 1);
    return 0;
}

static void try_create(IPluginFactory* factory, const char* label, const TUID cid, int which,
                       DWORD ms) {
    Job job = {};
    job.factory = factory;
    job.which = which;
    memcpy(job.cid, cid, sizeof(TUID));

    HANDLE thread = CreateThread(nullptr, 0, worker, &job, 0, nullptr);
    if (!thread) {
        printf("%s: CreateThread failed=%lu\n", label, GetLastError());
        return;
    }
    DWORD waited = WaitForSingleObject(thread, ms);
    if (waited != WAIT_TIMEOUT) {
        printf("%s: returned result=0x%x object=%p\n", label, job.result, job.object);
        CloseHandle(thread);
        return;
    }

    SuspendThread(thread);
    CONTEXT ctx = {};
    ctx.ContextFlags = CONTEXT_CONTROL | CONTEXT_INTEGER;
    GetThreadContext(thread, &ctx);
    auto base = reinterpret_cast<DWORD64>(reinterpret_cast<uintptr_t>(g_module));
    printf("%s: STILL RUNNING after %lu ms  rip rva=%#llx  rsp=%p\n", label, ms,
           static_cast<unsigned long long>(ctx.Rip - base), reinterpret_cast<void*>(ctx.Rsp));

    HMODULE rip_module = nullptr;
    if (GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS |
                               GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                           reinterpret_cast<LPCWSTR>(static_cast<uintptr_t>(ctx.Rip)),
                           &rip_module)) {
        wchar_t rip_name[MAX_PATH] = {};
        GetModuleFileNameW(rip_module, rip_name, MAX_PATH);
        printf("   rip module=%ls offset=%#llx\n", rip_name,
               static_cast<unsigned long long>(
                   ctx.Rip - reinterpret_cast<DWORD64>(reinterpret_cast<uintptr_t>(rip_module))));
    }

    DWORD tid = GetThreadId(thread);
    {
        auto base_bytes = reinterpret_cast<unsigned char*>(g_module);
        auto* cs = reinterpret_cast<const unsigned char*>(base_bytes + 0x2779db0);
        int lock_count = *reinterpret_cast<const int*>(cs + 0x08);
        int recursion = *reinterpret_cast<const int*>(cs + 0x0c);
        DWORD64 owner = *reinterpret_cast<const DWORD64*>(cs + 0x10);
        DWORD64 semaphore = *reinterpret_cast<const DWORD64*>(cs + 0x18);
        printf("   critical section 0x2779db0: lockCount=%d recursion=%d owner=%#llx sem=%#llx\n",
               lock_count, recursion, static_cast<unsigned long long>(owner),
               static_cast<unsigned long long>(semaphore));
        if (owner != 0 && owner != tid) {
            HANDLE other = OpenThread(THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_QUERY_INFORMATION,
                                      FALSE, static_cast<DWORD>(owner));
            if (other) {
                DWORD previous = SuspendThread(other);
                CONTEXT octx = {};
                octx.ContextFlags = CONTEXT_CONTROL | CONTEXT_INTEGER;
                if (GetThreadContext(other, &octx)) {
                    printf("   lock owner tid=%lu suspended(prev=%ld) rip rva=%#llx\n",
                           static_cast<unsigned long>(owner),
                           static_cast<long>(previous),
                           static_cast<unsigned long long>(octx.Rip - base));
                }
                ResumeThread(other);
                CloseHandle(other);
            } else {
                printf("   lock owner tid=%lu is gone (OpenThread=%lu)\n",
                       static_cast<unsigned long>(owner), GetLastError());
            }
        }
    }
    struct WindowScan {
        DWORD tid;
        int found;
    } scan = {tid, 0};
    EnumThreadWindows(
        tid,
        [](HWND window, LPARAM param) -> BOOL {
            auto* state = reinterpret_cast<WindowScan*>(param);
            wchar_t cls[128] = {};
            wchar_t title[256] = {};
            GetClassNameW(window, cls, 128);
            GetWindowTextW(window, title, 256);
            printf("   window class=%ls title=%ls\n", cls, title);
            state->found++;
            return TRUE;
        },
        reinterpret_cast<LPARAM>(&scan));
    if (scan.found == 0) {
        printf("   thread owns no windows\n");
    }

    auto* sp = reinterpret_cast<const DWORD64*>(reinterpret_cast<uintptr_t>(ctx.Rsp));
    for (int i = 0; i < 96; i++) {
        DWORD64 value = sp[i];
        if (value >= base && value < base + 0x9000000ULL) {
            printf("   stack[%2d] return rva=%#llx\n", i,
                   static_cast<unsigned long long>(value - base));
        }
    }
    TerminateThread(thread, 0);
    CloseHandle(thread);
}

static void enumerate(IPluginFactory* factory) {
    int classes = factory->countClasses();
    printf("classes=%d\n", classes);
    IPluginFactory3* factory3 = nullptr;
    factory->queryInterface(IPluginFactory3::iid, reinterpret_cast<void**>(&factory3));
    for (int i = 0; i < classes; i++) {
        if (!factory3) {
            break;
        }
        PClassInfoW info;
        memset(&info, 0, sizeof info);
        if (factory3->getClassInfoUnicode(i, &info) != kResultOk) {
            continue;
        }
        char name[256] = {};
        for (int c = 0; c < 255 && info.name[c]; c++) {
            name[c] = info.name[c] < 128 ? static_cast<char>(info.name[c]) : '?';
        }
        printf("class[%d] \"%s\" / \"%s\" cardinality=%d cid0=%02x%02x%02x%02x\n", i, name,
               info.category, info.cardinality, static_cast<unsigned char>(info.cid[0]),
               static_cast<unsigned char>(info.cid[1]), static_cast<unsigned char>(info.cid[2]),
               static_cast<unsigned char>(info.cid[3]));
    }
    if (factory3) {
        factory3->release();
    }
}

int main(int argc, char** argv) {
    setvbuf(stdout, nullptr, _IONBF, 0);
    AddVectoredExceptionHandler(1, on_exception);

    const char* mode = argc > 2 ? argv[2] : "enum";
    wchar_t path[MAX_PATH];
    MultiByteToWideChar(CP_UTF8, 0, argc > 1 ? argv[1] : "soothe3-native.vst3", -1, path,
                        MAX_PATH);

    HMODULE module = LoadLibraryW(path);
    if (!module) {
        printf("LoadLibrary failed=%lu\n", GetLastError());
        return 2;
    }
    g_module = module;
    printf("loaded=1 mode=%s\n", mode);

    auto init = reinterpret_cast<bool (*)()>(GetProcAddress(module, "InitDll"));
    auto factory_of =
        reinterpret_cast<IPluginFactory* (*)()>(GetProcAddress(module, "GetPluginFactory"));
    if (!init || !factory_of) {
        printf("missing exports\n");
        return 3;
    }
    printf("InitDll=%d\n", init());
    IPluginFactory* factory = factory_of();
    if (!factory) {
        printf("no factory\n");
        return 4;
    }

    TUID zero = {};
    TUID bogus = {};
    for (int i = 0; i < 16; i++) {
        bogus[i] = static_cast<char>(0x11 + i * 0x0b);
    }

    // Static constructors at 0x2ffbc0 / 0x2ffbf0 call InitializeCriticalSection
    // and then tail-jump into the proxy VM. If that hand-off never happens the
    // sections are left zero-initialised and the first EnterCriticalSection
    // deadlocks (LockCount 0 instead of -1).
    {
        auto module_base = reinterpret_cast<unsigned char*>(g_module);
        static const unsigned kLocks[] = {
            0x1dd30c0u, 0x205bce0u, 0x26d6700u, 0x26d7988u, 0x26d79c0u,
            0x26d79f8u, 0x26d7a30u, 0x2772e30u, 0x2777e60u, 0x2779910u,
            0x2779b70u, 0x2779ce8u, 0x2779d20u, 0x2779d78u, 0x2779db0u,
            0x2779f60u,
        };
        for (unsigned offset : kLocks) {
            auto* cs = reinterpret_cast<CRITICAL_SECTION*>(module_base + offset);
            if (cs->LockCount != 0 || cs->OwningThread != nullptr) {
                continue;
            }
            printf("cs 0x%x uninitialised -> fixing\n", offset);
            if (cs->LockCount == 0 && cs->OwningThread == nullptr) {
                InitializeCriticalSection(cs);
                printf("cs 0x%x initialised by hand -> lockCount=%ld\n", offset, cs->LockCount);
            }
        }
    }

    if (strcmp(mode, "enum") == 0) {
        enumerate(factory);
        printf("enumeration done; creating component with the reported (zero) cid\n");
        try_create(factory, "component/zero", zero, 0, 5000);
        try_create(factory, "controller/zero", zero, 1, 5000);
    } else if (strcmp(mode, "direct") == 0) {
        printf("no enumeration; creating component with the zero cid\n");
        try_create(factory, "component/zero", zero, 0, 5000);
    } else if (strcmp(mode, "rand") == 0) {
        printf("no enumeration; creating component with a bogus cid\n");
        try_create(factory, "component/bogus", bogus, 0, 5000);
    } else {
        printf("unknown mode\n");
    }

    factory->release();
    return 0;
}
