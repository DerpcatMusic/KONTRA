// Driver-free actual-shim ownership regression (no renderer or host process):
// g++ -std=c++17 -Wall -Wextra -Werror -pedantic vendor/moose-vst3/tests/editor_lifetime.cpp -o /tmp/kontra-editor-lifetime
// /tmp/kontra-editor-lifetime
#include "../shim/vst3_shim.cpp"

static bool context_alive, editor_open;
static int destroys, opens, closes;
static void require_impl(bool ok, const char* condition, int line) {
    if (!ok) { std::fprintf(stderr, "FAIL line %d: %s\n", line, condition); std::abort(); }
}
#define require(value) require_impl((value), #value, __LINE__)

struct RunLoop {
    void** vtbl;
    int refs = 1;
    int registrations = 0;
};
static uint32 loop_add_ref(void* self) { return ++static_cast<RunLoop*>(self)->refs; }
static uint32 loop_release(void* self) { return --static_cast<RunLoop*>(self)->refs; }
static tresult loop_register(void* self, void*, uint64_t) { ++static_cast<RunLoop*>(self)->registrations; return kResultOk; }
static tresult loop_unregister(void* self, void*) { --static_cast<RunLoop*>(self)->registrations; return kResultOk; }

struct Frame {
    void** vtbl;
    int refs = 1;
    int resizes = 0;
    RunLoop* loop = nullptr;
};
static tresult frame_query(void* self, const TUID iid, void** out) {
    auto* frame = static_cast<Frame*>(self);
    if (frame->loop && iid_equal(iid, IRunLoop_iid)) {
        *out = frame->loop; loop_add_ref(frame->loop); return kResultOk;
    }
    *out = nullptr; return kNoInterface;
}
static uint32 frame_add_ref(void* self) {
    auto* frame = static_cast<Frame*>(self); require(frame->refs > 0); return ++frame->refs;
}
static uint32 frame_release(void* self) {
    auto* frame = static_cast<Frame*>(self); require(frame->refs > 0); return --frame->refs;
}
static tresult frame_resize(void* self, void*, ViewRect* rect) {
    auto* frame = static_cast<Frame*>(self); require(frame->refs > 0);
    require(rect->right == 1180 && rect->bottom == 760); ++frame->resizes; return kResultOk;
}

int main() {
    Vst3Callbacks callbacks{};
    callbacks.create = []() -> void* { require(!context_alive); context_alive = true; return &context_alive; };
    callbacks.destroy = [](void*) { require(context_alive); context_alive = false; ++destroys; };
    callbacks.gui_has_editor = [](void*) -> int32_t { require(context_alive); return 1; };
    callbacks.gui_open = [](void*, void*) { require(context_alive); editor_open = true; ++opens; };
    callbacks.gui_close = [](void*) { require(context_alive && editor_open); editor_open = false; ++closes; };
    callbacks.gui_get_size = [](void*, uint32_t* w, uint32_t* h) { require(context_alive); *w = 1180; *h = 760; };
    g_cb = &callbacks;
    int parent_token;

    for (int iteration = 0; iteration != 100; ++iteration) {
        auto* com = create_component();
        require(com != nullptr);
        // Two outstanding views must each keep the shared context alive.
        auto* first = static_cast<MoosePlugView*>(com->vtbl_controller->createView(&com->vtbl_controller, "editor"));
        auto* current = static_cast<MoosePlugView*>(com->vtbl_controller->createView(&com->vtbl_controller, "editor"));
        require(first && current);
        void* scale = nullptr;
        require(current->vtbl->queryInterface(current, IPlugViewContentScaleSupport_iid, &scale) == kResultOk);
        // Host releases its last component interface before its plug views.
        com->vtbl_component->release(com);
        require(context_alive && destroys == iteration);
        require(first->vtbl->release(first) == 0);
        require(com->impl.plugView == current);
        require(current->vtbl->attached(current, &parent_token, kPlatformTypeX11) == kResultOk);
        require(current->vtbl->removed(current) == kResultOk);
        require(current->vtbl->attached(current, &parent_token, kPlatformTypeX11) == kResultOk);
        require(current->vtbl->removed(current) == kResultOk);
        require(current->vtbl->release(current) == 1);
        // The secondary scale interface keeps the same view and owner alive.
        ViewRect size{};
        require(pv_getSize(pv_from_scale(scale), &size) == kResultOk && size.right == 1180);
        require(pvcs_release(scale) == 0);
        require(!context_alive && destroys == iteration + 1);
    }
    require(opens == 200 && closes == 200);
    // Also cover the common order: view released before component.
    auto* com = create_component();
    auto* view = static_cast<MoosePlugView*>(com->vtbl_controller->createView(&com->vtbl_controller, "editor"));
    require(view && view->vtbl->release(view) == 0 && context_alive);
    require(com->vtbl_component->release(com) == 0 && !context_alive && destroys == 101);
    // IPlugFrame is another independent COM owner. The host may release its
    // reference after setFrame; retain it until replacement, clear or teardown.
    void* frame_vtbl[] = {reinterpret_cast<void*>(frame_query), reinterpret_cast<void*>(frame_add_ref),
        reinterpret_cast<void*>(frame_release), reinterpret_cast<void*>(frame_resize)};
    Frame first_frame{frame_vtbl}, second_frame{frame_vtbl}, final_frame{frame_vtbl};
    com = create_component();
    view = static_cast<MoosePlugView*>(com->vtbl_controller->createView(&com->vtbl_controller, "editor"));
    require(view->vtbl->setFrame(view, &first_frame) == kResultOk && first_frame.refs == 2);
    require(view->vtbl->setFrame(view, &first_frame) == kResultOk && first_frame.refs == 2);
    require(frame_release(&first_frame) == 1);
    require(moose_vst3_request_resize(com->impl.rustContext(), 1180, 760) == 1 && first_frame.resizes == 1);
    require(view->vtbl->setFrame(view, &second_frame) == kResultOk && first_frame.refs == 0 && second_frame.refs == 2);
    require(frame_release(&second_frame) == 1);
    require(view->vtbl->setFrame(view, nullptr) == kResultOk && second_frame.refs == 0);
    require(moose_vst3_request_resize(com->impl.rustContext(), 1180, 760) == 0);
    require(view->vtbl->setFrame(view, &final_frame) == kResultOk && final_frame.refs == 2);
    require(frame_release(&final_frame) == 1);
    require(view->vtbl->release(view) == 0 && final_frame.refs == 0 && context_alive);
    require(com->vtbl_component->release(com) == 0 && !context_alive && destroys == 102);
    void* loop_vtbl[] = {nullptr, reinterpret_cast<void*>(loop_add_ref), reinterpret_cast<void*>(loop_release),
        nullptr, nullptr, reinterpret_cast<void*>(loop_register), reinterpret_cast<void*>(loop_unregister)};
    RunLoop old_loop{loop_vtbl}, current_loop{loop_vtbl};
    Frame old_frame{frame_vtbl, 1, 0, &old_loop}, current_frame{frame_vtbl, 1, 0, &current_loop};
    com = create_component();
    auto* old = static_cast<MoosePlugView*>(com->vtbl_controller->createView(&com->vtbl_controller, "editor"));
    require(old->vtbl->setFrame(old, &old_frame) == kResultOk && old_loop.registrations == 1);
    view = static_cast<MoosePlugView*>(com->vtbl_controller->createView(&com->vtbl_controller, "editor"));
    require(view->vtbl->setFrame(view, &current_frame) == kResultOk && current_loop.registrations == 1);
    require(old_loop.registrations == 0 && old_loop.refs == 1);
    require(old->vtbl->setFrame(old, nullptr) == kResultOk && current_loop.registrations == 1);
    require(old->vtbl->setFrame(old, &old_frame) == kResultOk && current_loop.registrations == 1 && old_loop.registrations == 0);
    require(old->vtbl->release(old) == 0 && current_loop.registrations == 1 && current_loop.refs == 2);
    require(view->vtbl->release(view) == 0 && current_loop.registrations == 0 && current_loop.refs == 1);
    require(old_frame.refs == 1 && current_frame.refs == 1);
    require(com->vtbl_component->release(com) == 0 && !context_alive && destroys == 103);
    const int opens_before = opens, closes_before = closes;
    com = create_component();
    old = static_cast<MoosePlugView*>(com->vtbl_controller->createView(&com->vtbl_controller, "editor"));
    require(old->vtbl->attached(old, &parent_token, kPlatformTypeX11) == kResultOk && editor_open);
    view = static_cast<MoosePlugView*>(com->vtbl_controller->createView(&com->vtbl_controller, "editor"));
    // Merely creating a replacement must not disown the still-attached view.
    require(old->vtbl->removed(old) == kResultOk && !editor_open && closes == closes_before + 1);
    require(old->vtbl->attached(old, &parent_token, kPlatformTypeX11) == kResultOk && editor_open);
    require(view->vtbl->attached(view, &parent_token, kPlatformTypeX11) == kResultOk && editor_open);
    // New attachment supersedes old ownership. Old removal/release must not
    // close the new native editor sharing this Rust context.
    require(old->vtbl->removed(old) == kResultOk && editor_open && closes == closes_before + 1);
    require(old->vtbl->release(old) == 0 && editor_open && closes == closes_before + 1);
    require(view->vtbl->removed(view) == kResultOk && !editor_open && closes == closes_before + 2);
    require(view->vtbl->removed(view) == kResultOk && closes == closes_before + 2);
    require(view->vtbl->attached(view, &parent_token, kPlatformTypeX11) == kResultOk && editor_open);
    require(com->vtbl_component->release(com) == 1 && context_alive);
    require(view->vtbl->release(view) == 0 && !editor_open && !context_alive && destroys == 104);
    require(opens == opens_before + 4 && closes == closes_before + 3);
    std::puts("PASS: component/view/frame/run-loop ownership; attachment-owned GUI survives old-view removal/release");
}
