// List vtables that share given slot targets: the classes derived from a base.
//   RE_SCRIPT=FindVtables.java tools/re/query.sh 0x144f00000 0x144fff000 0x30=0x140ac17a0 0x38=0x140ab9a30
// Args: <from> <to> then `<byte offset>=<target>` pairs. Output: `vtable <address>
// slot23 <target>` per match.
// @category KONTRA
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.mem.Memory;

public class FindVtables extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        long from = Long.decode(args[0]), to = Long.decode(args[1]);
        Memory mem = currentProgram.getMemory();
        for (long a = from; a < to && !monitor.isCancelled(); a += 8) {
            Address at = toAddr(a);
            boolean ok = true;
            for (int i = 2; i < args.length && ok; i++) {
                String[] kv = args[i].split("=");
                try {
                    ok = mem.getLong(at.add(Long.decode(kv[0]))) == Long.decode(kv[1]);
                } catch (Exception e) {
                    ok = false;
                }
            }
            if (ok) {
                long slot23 = mem.getLong(at.add(0xb8));
                println(String.format("vtable 0x%x slot23 0x%x", a, slot23));
            }
        }
    }
}
