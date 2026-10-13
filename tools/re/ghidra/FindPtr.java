// Find 8-byte little-endian occurrences of an address in memory (vtable slots holding a function).
//   RE_SCRIPT=FindPtr.java tools/re/query.sh 0x140aa1130
// Args: <value>. Output: `ptr at <address>` per hit, with the previous and next 3 qwords for context.
// @category KONTRA
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.mem.Memory;

public class FindPtr extends GhidraScript {
    @Override
    public void run() throws Exception {
        long v = toAddr(getScriptArgs()[0]).getOffset();
        byte[] ptr = new byte[8];
        for (int i = 0; i < 8; i++) ptr[i] = (byte) (v >>> (8 * i));
        Memory mem = currentProgram.getMemory();
        Address p = mem.getMinAddress();
        while ((p = mem.findBytes(p, ptr, null, true, monitor)) != null) {
            println("ptr at " + p);
            p = p.add(1);
        }
    }
}
