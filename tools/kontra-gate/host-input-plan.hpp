// Reused bounded input parser from W11 130ca844; GUI-only lifecycle does not reload plugins.
#pragma once
#include <array>
#include <cstdint>
#include <limits>
#include <sstream>
#include <stdexcept>
#include <string>

namespace widget_host {
enum class Op { Move, Button, Key, Wait, Checkpoint };
struct Command {
    uint32_t sequence;
    Op op;
    std::array<int64_t, 4> args{};
};
inline Command parse(const std::string &line) {
    if (line.empty() || line.size() > 256)
        throw std::runtime_error("plan line bound");
    std::istringstream input(line);
    int64_t sequence;
    std::string op;
    if (!(input >> sequence >> op) || sequence < 0 || sequence > 4095)
        throw std::runtime_error("plan sequence");
    Command command{uint32_t(sequence), Op::Checkpoint};
    unsigned count = 0;
    if (op == "move") {
        command.op = Op::Move;
        count = 2;
    } else if (op == "button") {
        command.op = Op::Button;
        count = 2;
    } else if (op == "key") {
        command.op = Op::Key;
        count = 2;
    } else if (op == "wait") {
        command.op = Op::Wait;
        count = 1;
    } else if (op == "checkpoint")
        command.op = Op::Checkpoint;
    else
        throw std::runtime_error("plan operation");
    for (unsigned i = 0; i < count; ++i)
        if (!(input >> command.args[i]))
            throw std::runtime_error("plan argument");
    std::string extra;
    if (input >> extra)
        throw std::runtime_error("plan trailing argument");
    const auto &a = command.args;
    if (command.op == Op::Move && (a[0] < 0 || a[0] > 4095 || a[1] < 0 || a[1] > 2159))
        throw std::runtime_error("plan coordinate");
    if (command.op == Op::Button && (a[0] < 1 || a[0] > 5 || (a[1] != 0 && a[1] != 1)))
        throw std::runtime_error("plan button");
    if (command.op == Op::Key &&
        (a[0] < 1 || a[0] > std::numeric_limits<uint32_t>::max() || (a[1] != 0 && a[1] != 1)))
        throw std::runtime_error("plan key");
    if (command.op == Op::Wait && (a[0] < 0 || a[0] > 10000))
        throw std::runtime_error("plan wait");
    return command;
}
inline std::array<int, 2> root_point(const Command &command, int width, int height, int root_x,
                                     int root_y) {
    if (command.op != Op::Move || width <= 0 || height <= 0 || command.args[0] >= width ||
        command.args[1] >= height)
        throw std::runtime_error("editor coordinate bound");
    return {root_x + int(command.args[0]), root_y + int(command.args[1])};
}
} // namespace widget_host
