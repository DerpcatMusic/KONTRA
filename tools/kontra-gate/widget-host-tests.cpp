#include "widget-host-plan.hpp"
#include <algorithm>
#include <functional>
#include <iostream>
using namespace widget_host;
static void check(bool ok, const char *message) {
    if (!ok)
        throw std::runtime_error(message);
}
static void rejects(const std::function<void()> &run) {
    try {
        run();
    } catch (const std::runtime_error &) {
        return;
    }
    throw std::runtime_error("invalid plan accepted");
}
int main() {
    unsigned tests = 0;
    try {
        for (const auto *line :
             {"0 move 12 34", "1 button 1 1", "2 key 65507 0", "3 wait 100", "4 checkpoint",
              "5 save", "6 reload", "7 midi 0 144 60 100", "8 quit"})
            parse(line);
        ++tests;
        for (const auto *line :
             {"", "-1 quit", "4096 quit", "0 callback 1", "0 move 1", "0 move -1 0",
              "0 move 4096 0", "0 move 0 2160", "0 button 0 1", "0 button 1 2", "0 key 0 1",
              "0 key 4294967296 1", "0 wait 10001", "0 midi 0 144 128 100", "0 midi 0 255 60 100",
              "0 quit extra", "0 save 1"})
            rejects([&] { parse(line); });
        rejects([] { parse(std::string(257, '1')); });
        ++tests;
        const auto at = root_point(parse("0 move 10 20"), 100, 100, 300, 400);
        check(at[0] == 310 && at[1] == 420, "child-relative translation");
        rejects([] { root_point(parse("0 move 100 0"), 100, 100, 0, 0); });
        rejects([] { root_point(parse("0 move 0 0"), 0, 100, 0, 0); });
        ++tests;
        struct Fake {
            std::vector<std::string> calls;
            int instance = 1, value = 7, saved = 7;
            bool audio = true, editor = true, held = true;
            void release_inputs() {
                held = false;
                calls.push_back("release");
            }
            void stop_audio() {
                check(!held, "inputs held before stop");
                audio = false;
                calls.push_back("stop");
            }
            void close_editor() {
                check(!audio, "audio before editor destruction");
                editor = false;
                calls.push_back("close");
            }
            void destroy_plugin() {
                check(!audio && !editor, "live instance destruction");
                value = -1;
                calls.push_back("destroy");
            }
            void reset_host() { calls.push_back("reset"); }
            void create_plugin() {
                ++instance;
                value = 0;
                calls.push_back("create");
            }
            void load_saved_state() {
                check(instance == 2 && value == 0 && !audio, "load into fresh inactive plugin");
                value = saved;
                calls.push_back("load");
            }
            void start_audio() {
                check(value == saved, "start before restore");
                audio = true;
                calls.push_back("start");
            }
            void await_ready() {
                check(audio, "readiness before processing");
                calls.push_back("ready");
            }
            void open_editor() {
                check(audio, "editor before processing");
                editor = true;
                calls.push_back("open");
            }
        } fake;
        fresh_reload(fake);
        check(fake.instance == 2 && fake.value == 7 && fake.editor && fake.audio && !fake.held,
              "fresh-plugin recall");
        check(fake.calls ==
                  std::vector<std::string>({"release", "stop", "close", "destroy", "reset",
                                            "create", "load", "start", "ready", "open"}),
              "lifecycle order");
        ++tests;
        std::cout << "{\"transport_tests_passed\":" << tests << "}\n";
    } catch (const std::exception &e) {
        std::cerr << e.what() << '\n';
        return 1;
    }
}
