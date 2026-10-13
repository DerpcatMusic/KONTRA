// List functions whose instructions use ALL of the given displacements/constants as
// memory-operand offsets (find a class's constructor or setter from its field offsets).
//   RE_SCRIPT=DispScan.java tools/re/query.sh 0x270 0x2b0 0x230
// @category KONTRA
import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.Instruction;
import ghidra.program.model.scalar.Scalar;
import java.util.*;

public class DispScan extends GhidraScript {
    @Override
    public void run() throws Exception {
        Set<Long> want = new HashSet<>();
        for (String a : getScriptArgs()) want.add(Long.decode(a));
        Map<String, Set<Long>> hits = new TreeMap<>();
        for (Instruction ins : currentProgram.getListing().getInstructions(true)) {
            if (monitor.isCancelled()) break;
            for (int i = 0; i < ins.getNumOperands(); i++) {
                for (Object o : ins.getOpObjects(i)) {
                    if (o instanceof Scalar s && want.contains(s.getValue())) {
                        Function f = getFunctionContaining(ins.getAddress());
                        if (f != null) {
                            hits.computeIfAbsent(f.getEntryPoint() + " " + f.getName(), k -> new TreeSet<>()).add(s.getValue());
                        }
                    }
                }
            }
        }
        for (var e : hits.entrySet()) if (e.getValue().size() >= want.size()) println(e.getKey() + " " + e.getValue());
    }
}
