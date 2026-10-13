// Print consecutive 8-byte pointers (vtables, jump tables).
//   RE_SCRIPT=ReadPtrs.java tools/re/query.sh 0x144e74260 12
// Args: <address> [count, default 16]. Output: `<addr> -> <value>` per slot.
// @category KONTRA
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;

public class ReadPtrs extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] a = getScriptArgs();
        Address p = toAddr(a[0]);
        int n = a.length > 1 ? Integer.parseInt(a[1]) : 16;
        for (int i = 0; i < n; i++) {
            println(p.add(8L * i) + " (+0x" + Integer.toHexString(8 * i) + ") -> " + Long.toHexString(currentProgram.getMemory().getLong(p.add(8L * i))));
        }
    }
}
