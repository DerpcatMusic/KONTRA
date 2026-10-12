#!/usr/bin/env python3
"""Check the documented native legacy-EQ laws against frozen numeric vectors."""
import json
import math
import struct
from pathlib import Path


def f(x):
    return struct.unpack('<f', struct.pack('<f', x))[0]


def exp_bits(z):
    q = f(z - math.floor(z))
    bits = int(f(f(f(z + 127) - f(f(q - f(q * q)) * f(.33971))) * 8388608))
    return struct.unpack('<f', struct.pack('<I', bits))[0]


def saved(hz, octaves, db):
    u = min(1, max(0, f(f(hz - 20) * f(.000050050050049321726))))
    t = f(f(u * 468.7109375) + 1)
    bits = struct.unpack('<i', struct.pack('<f', t))[0]
    z = f(f(bits * 2 ** -23) - 127)
    q = f(z - math.floor(z))
    cutoff = f(f(z + f(f(q - f(q * q)) * f(.346607))) * f(.11257956176996231))
    width = f(f(octaves - f(1 / 3)) * f(.375))
    gain = f(f(f(f(db + 18) * f(1 / 36)) - .5) * 2)
    return cutoff, width, gain


def coefficients(rate, cutoff, width, gain):
    u = f(f(exp_bits(f(cutoff * f(8.882606506347656))) - 1) * f(.0021335112396627665))
    hz = f(f(min(1, max(0, u)) * 19980) + 20)
    omega = f(min(f(hz / rate), f(.49)) * math.tau)
    s, c = f(math.sin(omega)), f(math.cos(omega))
    a = exp_bits(f(f(f(f(gain * .5) + .5) * 6) - 3))
    b = 2 ** f(f(width * f(2.6666667461395264)) + f(1 / 3))
    alpha = s * (.5 / (1 / (b - 1 / b)))
    p, q = alpha / a, alpha * a
    d = 1 / (1 + p)
    return hz, [f((q + 1) * d), f(-2 * c * d), f((1 - q) * d), f(2 * c * d), f((p - 1) * d)]


def main():
    vectors = json.loads((Path(__file__).resolve().parents[2] /
        'docs/architecture-v2/KONTAKT_DSP_LAWS.vectors.json').read_text())
    saved_error = max(abs(a - b) for row in vectors['eq_saved_conversion']
        for a, b in zip(saved(row['hz'], row['octaves'], row['db']), row['normalized']))
    coefficient_error = 0
    for row in vectors['eq_coefficients']:
        hz, actual = coefficients(row['rate'], row['cutoff'], row['width'], row['gain'])
        assert abs(hz - row['frequency']) < .003
        coefficient_error = max(coefficient_error,
            max(abs(a - b) for a, b in zip(actual, row['coefficients'])))
    assert saved_error <= 2e-7, saved_error
    assert coefficient_error <= 2e-6, coefficient_error
    print(json.dumps({'saved_cases': len(vectors['eq_saved_conversion']),
        'coefficient_cases': len(vectors['eq_coefficients']),
        'saved_max_abs_error': saved_error,
        'coefficient_max_abs_error': coefficient_error}))


if __name__ == '__main__':
    main()
