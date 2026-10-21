// Print the bytes at addresses as f32 / f64 / u32 (constant pools: `_DAT_144714930`).
//   RE_SCRIPT=ReadFloats.java tools/re/query.sh 0x144714930 0x144f38804 ...
// Args: addresses. Output: `<addr> f32=<v> bits=0x<hex> f64=<v>` per address.
// @category KONTRA
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;

public class ReadFloats extends GhidraScript {
    @Override
    public void run() throws Exception {
        for (String a : getScriptArgs()) {
            Address p = toAddr(a);
            int bits = currentProgram.getMemory().getInt(p);
            long wide = currentProgram.getMemory().getLong(p);
            println(String.format("%s f32=%.9g bits=0x%08x f64=%.17g", a, Float.intBitsToFloat(bits), bits, Double.longBitsToDouble(wide)));
        }
    }
}
