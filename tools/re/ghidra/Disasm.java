// Print instructions in an address range.
//   RE_SCRIPT=Disasm.java tools/re/query.sh 0x14156bc40 0x14156bd60
// Args: <start> <end>. Output: `address  instruction` per line.
// @category KONTRA
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Instruction;

public class Disasm extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        Address from = toAddr(args[0]);
        Address to = toAddr(args[1]);
        Instruction ins = getInstructionAt(from);
        if (ins == null) {
            disassemble(from);
            ins = getInstructionAt(from);
        }
        while (ins != null && ins.getAddress().compareTo(to) < 0) {
            println(ins.getAddress() + "  " + ins);
            ins = ins.getNext();
        }
    }
}
