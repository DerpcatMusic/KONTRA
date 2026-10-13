// List the functions whose instructions access every given object offset ([reg + 0xNNN]).
//   RE_SCRIPT=FieldRefs.java tools/re/query.sh 0x2c4,0x2c8 [rangeLow rangeHigh]
// Args: comma-separated hex offsets; optional address range. Output: `fn <entry> <name> hits=N` for
// functions that touch ALL the offsets (writes marked with W). Operand text match, so keep offsets distinctive.
// @category KONTRA
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.Instruction;
import java.util.*;

public class FieldRefs extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        String[] offs = args[0].split(",");
        Address lo = args.length > 2 ? toAddr(args[1]) : currentProgram.getMinAddress();
        Address hi = args.length > 2 ? toAddr(args[2]) : currentProgram.getMaxAddress();
        Map<Function, Set<String>> seen = new LinkedHashMap<>();
        Map<Function, Set<String>> writes = new HashMap<>();
        for (Instruction ins : currentProgram.getListing().getInstructions(lo, true)) {
            if (monitor.isCancelled() || ins.getAddress().compareTo(hi) > 0) break;
            String text = ins.toString();
            for (String o : offs) {
                if (text.contains("+ " + o + "]") || text.contains("+ " + o + ",")) {
                    Function f = getFunctionContaining(ins.getAddress());
                    if (f == null) continue;
                    seen.computeIfAbsent(f, k -> new TreeSet<>()).add(o);
                    if (text.startsWith("MOV") && text.indexOf('[') < text.indexOf(',')) {
                        writes.computeIfAbsent(f, k -> new TreeSet<>()).add(o);
                    }
                }
            }
        }
        for (Map.Entry<Function, Set<String>> e : seen.entrySet()) {
            if (e.getValue().size() == offs.length) {
                println("fn " + e.getKey().getEntryPoint() + " " + e.getKey().getName() + " writes=" + writes.getOrDefault(e.getKey(), Set.of()));
            }
        }
    }
}
