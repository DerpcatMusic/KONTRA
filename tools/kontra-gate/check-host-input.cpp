#include "host-input-plan.hpp"
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
             {"0 move 12 34", "1 button 1 1", "2 key 65507 0", "3 wait 100", "4 checkpoint"})
            parse(line);
        ++tests;
        for (const auto *line :
             {"", "-1 checkpoint", "4096 checkpoint", "0 callback 1", "0 move 1", "0 move -1 0",
              "0 move 4096 0", "0 move 0 2160", "0 button 0 1", "0 button 1 2", "0 key 0 1",
              "0 key 4294967296 1", "0 wait 10001", "0 midi 0 144 128 100", "0 midi 0 255 60 100",
              "0 checkpoint extra", "0 save", "0 reload", "0 quit", "0 midi 0 144 60 100"})
            rejects([&] { parse(line); });
        rejects([] { parse(std::string(257, '1')); });
        ++tests;
        const auto at = root_point(parse("0 move 10 20"), 100, 100, 300, 400);
        check(at[0] == 310 && at[1] == 420, "child-relative translation");
        rejects([] { root_point(parse("0 move 100 0"), 100, 100, 0, 0); });
        rejects([] { root_point(parse("0 move 0 0"), 0, 100, 0, 0); });
        ++tests;
        std::cout << "{\"transport_tests_passed\":" << tests << "}\n";
    } catch (const std::exception &e) {
        std::cerr << e.what() << '\n';
        return 1;
    }
}
