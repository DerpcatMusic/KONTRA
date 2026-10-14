// List the instructions that use an immediate, with their function: finds the
// constructor of a class from its object size (e.g. `operator new(0x1478)`).
//   RE_SCRIPT=FindImm.java tools/re/query.sh 0x1478
// Args: <immediate>. Output: `imm at <address> in <function>` per instruction.
// @category KONTRA
import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.Instruction;
import ghidra.program.model.scalar.Scalar;

public class FindImm extends GhidraScript {
    @Override
    public void run() throws Exception {
        long want = Long.decode(getScriptArgs()[0]);
        for (Instruction ins : currentProgram.getListing().getInstructions(true)) {
            if (monitor.isCancelled()) break;
            for (int i = 0; i < ins.getNumOperands(); i++) {
                for (Object o : ins.getOpObjects(i)) {
                    if (o instanceof Scalar s && s.getUnsignedValue() == want) {
                        Function f = getFunctionContaining(ins.getAddress());
                        println("imm at " + ins.getAddress() + " in " + (f == null ? "<none>" : f.getEntryPoint() + " " + f.getName()) + "  " + ins);
                    }
                }
            }
        }
    }
}
