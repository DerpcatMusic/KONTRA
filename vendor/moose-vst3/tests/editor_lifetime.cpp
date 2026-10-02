// Driver-free actual-shim ownership regression (no renderer or host process):
// g++ -std=c++17 -Wall -Wextra -Werror -pedantic vendor/moose-vst3/tests/editor_lifetime.cpp -o /tmp/kontra-editor-lifetime
// /tmp/kontra-editor-lifetime
#include "../shim/vst3_shim.cpp"

static bool context_alive;
static int destroys, opens, closes;
static void require(bool ok) {
    if (!ok) { std::fputs("FAIL: editor/context ownership invariant\n", stderr); std::abort(); }
}

int main() {
    Vst3Callbacks callbacks{};
    callbacks.create = []() -> void* { require(!context_alive); context_alive = true; return &context_alive; };
    callbacks.destroy = [](void*) { require(context_alive); context_alive = false; ++destroys; };
    callbacks.gui_has_editor = [](void*) -> int32_t { require(context_alive); return 1; };
    callbacks.gui_open = [](void*, void*) { require(context_alive); ++opens; };
    callbacks.gui_close = [](void*) { require(context_alive); ++closes; };
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
    std::puts("PASS: component-first and view-first release, multiple views, reopen, scale interface, exactly-once teardown");
}
