// List the functions that reference an address (e.g. a vtable: its constructors).
//   RE_SCRIPT=RefsTo.java tools/re/query.sh 0x144e97640
// Args: <address>. Output: `ref from <address> in <function>` per reference.
// @category KONTRA
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.symbol.Reference;

public class RefsTo extends GhidraScript {
    @Override
    public void run() throws Exception {
        Address target = toAddr(getScriptArgs()[0]);
        for (Reference r : getReferencesTo(target)) {
            Function f = getFunctionContaining(r.getFromAddress());
            println("ref from " + r.getFromAddress() + " in " + (f == null ? "<none>" : f.getEntryPoint() + " " + f.getName()));
        }
        // RIP-relative LEA of the address without a recorded reference.
        long want = target.getOffset();
        for (ghidra.program.model.listing.Instruction ins : currentProgram.getListing().getInstructions(true)) {
            if (monitor.isCancelled()) break;
            if (!ins.getMnemonicString().equals("LEA")) continue;
            for (ghidra.program.model.pcode.PcodeOp op : ins.getPcode()) {
                for (ghidra.program.model.pcode.Varnode in : op.getInputs()) {
                    if (in.isConstant() && in.getOffset() == want) {
                        Function f = getFunctionContaining(ins.getAddress());
                        println("lea at " + ins.getAddress() + " in " + (f == null ? "<none>" : f.getEntryPoint() + " " + f.getName()));
                    }
                }
            }
        }
    }
}
