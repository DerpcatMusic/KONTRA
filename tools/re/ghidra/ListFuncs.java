// List function entry points in an address range.
//   RE_SCRIPT=ListFuncs.java tools/re/query.sh 0x140a73000 0x140a76000
// @category KONTRA
import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.Function;

public class ListFuncs extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] a = getScriptArgs();
        for (Function f : currentProgram.getFunctionManager().getFunctions(toAddr(a[0]), true)) {
            if (f.getEntryPoint().compareTo(toAddr(a[1])) > 0) break;
            println(f.getEntryPoint() + " " + f.getName() + " size=" + f.getBody().getNumAddresses());
        }
    }
}
