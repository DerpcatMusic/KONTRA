// Print the function pointers of a vtable.
//   RE_SCRIPT=DumpVtable.java tools/re/query.sh 0x144e93130 12
// Args: <vtable address> [slot count, default 12]. Output: one `slot offset target` line each.
// @category KONTRA
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;

public class DumpVtable extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        Address base = toAddr(args[0]);
        int n = args.length > 1 ? Integer.parseInt(args[1]) : 12;
        for (int i = 0; i < n; i++) {
            long target = currentProgram.getMemory().getLong(base.add(8L * i));
            Function f = getFunctionAt(toAddr(target));
            println(String.format("slot %d +0x%x -> 0x%x %s", i, 8 * i, target, f == null ? "" : f.getName()));
        }
    }
}
