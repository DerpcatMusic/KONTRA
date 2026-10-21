# The reference, executed

`unpacer run-kernel` runs a procedure from the restored image natively on
Linux, with a declared memory case: blocks of state, patches that point at
other blocks, and the call itself. The tool maps the sections the case names
(`.text`, `.rdata`, `.data` — a missing section is a segfault, not a zero),
calls the procedure through the Windows x64 ABI bridge, and records the
streams before and after.

This is a parity oracle: the reference's own arithmetic, on this machine.

## The case: soothe3's gain helper

`0x899ab0` is the per-sample gain ramp the numerical audit documents
(`soothe3-numerical-audit/equations.md`): a target, a current gain and a
coefficient in a 12-byte state, a channel descriptor with a pointer per
channel, and one multiply per sample per channel.

```
unpacer run-kernel soothe3-restored.bin \
    --case case-gain-helper.json --bundle ./bundle
```

with the state `{target 0.5, current 1.0, coefficient 0.05}`, two channels of
eight samples, and the call `{entry_rva 0x899ab0, args [state, descriptor]}`.

The run:

```
observation 0: state current=1.000000000
observation 1: state current=0.831710219
   left [0.24375, 0.475625, 0.696516, 0.907253, 1.108613, 1.301319, 1.486045, 1.66342]
```

and the audit's law, evaluated independently:

```
g <- ((t - g) * a) + g        for each sample, then x[c,n] *= g
gains  [0.975, 0.95125, 0.928687, 0.907253, 0.88689, 0.867546, 0.849169, 0.83171]
left   [0.24375, 0.475625, 0.696516, 0.907253, 1.108613, 1.301319, 1.486045, 1.66342]
```

The same numbers to the last bit: the audit's law *is* the target's
behaviour, and the port's ramp is checked against it.
