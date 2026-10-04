/* Recover one observed 64-bit UFS XOR-stream key from four known output words.
   No bank-specific names, keys, or ciphertext are embedded here. */
#define _POSIX_C_SOURCE 200809L
#include <fcntl.h>
#include <inttypes.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/stat.h>
#include <unistd.h>

static const uint64_t MURMUR_M = UINT64_C(0xc6a4a7935bd1e995);
static const uint64_t PCG_MULT = UINT64_C(0x5851f42d4c957f2d);

static inline uint64_t mix64(uint64_t value) {
    uint64_t mixed = value * MURMUR_M;
    return (mixed ^ (mixed >> 47)) * MURMUR_M;
}

static inline uint32_t output32(uint64_t state) {
    return (uint32_t)(((state >> 22) ^ state) >> (22 + (state >> 61)));
}

static int store_key(const char *path, uint64_t key) {
    int fd = open(path, O_WRONLY | O_CREAT | O_EXCL, S_IRUSR | S_IWUSR);
    if (fd < 0) return -1;
    uint8_t bytes[8];
    for (unsigned i = 0; i < 8; ++i) bytes[i] = (uint8_t)(key >> (8 * i));
    ssize_t written = write(fd, bytes, sizeof(bytes));
    int close_result = close(fd);
    if (written != (ssize_t)sizeof(bytes) || close_result != 0) {
        unlink(path);
        return -1;
    }
    return 0;
}

int main(int argc, char **argv) {
    if (argc != 3) {
        fprintf(stderr, "usage: uvi_recover_state OFFSET OUTPUT_FILE < four-words-on-stdin\n");
        return 2;
    }
    char *end = NULL;
    uint64_t offset = strtoull(argv[1], &end, 0);
    if (end == argv[1] || *end != '\0') return 2;
    uint32_t expected[4];
    for (unsigned i = 0; i < 4; ++i) {
        if (scanf("%" SCNu32, &expected[i]) != 1) {
            fprintf(stderr, "four output words are required\n");
            return 2;
        }
    }

    _Atomic int found = 0;
    _Atomic int write_failed = 0;
#ifdef _OPENMP
#pragma omp parallel for schedule(dynamic, 1)
#endif
    for (int task = 0; task < 255; ++task) {
        if (atomic_load_explicit(&found, memory_order_relaxed)) continue;
        unsigned h = 0;
        unsigned upper = (unsigned)task;
        while (upper >= (1u << (7u - h))) {
            upper -= 1u << (7u - h);
            ++h;
        }
        unsigned shift = 22u + h;
        uint64_t y = (uint64_t)expected[0] | ((uint64_t)upper << 32) |
                     ((uint64_t)h << (61u - shift));
        uint64_t base = (y ^ (y >> 22)) << shift;
        uint64_t state1 = base * PCG_MULT;
        uint64_t limit = UINT64_C(1) << shift;
        for (uint64_t low = 0; low < limit; ++low, state1 += PCG_MULT) {
            if (atomic_load_explicit(&found, memory_order_relaxed)) break;
            if (output32(state1) != expected[1]) continue;
            uint64_t state2 = state1 * PCG_MULT;
            if (output32(state2) != expected[2] ||
                output32(state2 * PCG_MULT) != expected[3]) continue;
            uint64_t initial = base | low;
            if (output32(initial) != expected[0]) continue;

            /* Invert multiplication by odd M modulo 2^64 with Newton steps. */
            uint64_t inverse = 1;
            for (unsigned i = 0; i < 6; ++i) inverse *= 2 - MURMUR_M * inverse;
            uint64_t key = (initial * inverse) ^ mix64(offset);
            int expected_found = 0;
            if (atomic_compare_exchange_strong(&found, &expected_found, 1)) {
                if (store_key(argv[2], key) != 0)
                    atomic_store(&write_failed, 1);
            }
            break;
        }
    }

    if (atomic_load(&write_failed)) {
        fprintf(stderr, "could not create the owner-only key file\n");
        return 1;
    }
    if (!atomic_load(&found)) {
        fprintf(stderr, "no matching stream state found\n");
        return 1;
    }
    return 0;
}
