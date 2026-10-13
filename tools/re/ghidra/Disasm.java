// Print instructions from an address (or a whole function when count is 0).
//   RE_SCRIPT=Disasm.java tools/re/query.sh 0x140962c60 40
// Args: <address> [count, 0 = the containing function]. Output: `<addr> <instruction>` per line.
// @category KONTRA
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.Instruction;

public class Disasm extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        Address at = toAddr(args[0]);
        int n = args.length > 2 ? 0 : args.length > 1 ? Integer.parseInt(args[1]) : 40;
        if (args.length > 2) {  // range sweep: <start> <end> <ignored>: disassemble gaps linearly
            Address stop = toAddr(args[1]);
            Address a = at;
            while (a.compareTo(stop) < 0) {
                Instruction i = getInstructionAt(a);
                if (i == null) { disassemble(a); i = getInstructionAt(a); }
                if (i == null) { a = a.add(1); continue; }
                println(i.getAddress() + " " + i);
                a = i.getAddress().add(i.getLength());
            }
            return;
        }
        if (n == 0 && args.length <= 2) {
            Function f = getFunctionContaining(at);
            if (f == null) { println("// no function"); return; }
            for (Instruction i : currentProgram.getListing().getInstructions(f.getBody(), true)) println(i.getAddress() + " " + i);
            return;
        }
        Address end = null;
        Instruction ins = getInstructionAt(at);
        if (ins == null) { disassemble(at); ins = getInstructionAt(at); }
        for (int i = 0; i < n && ins != null; i++) {
            if (end != null && ins.getAddress().compareTo(end) > 0) break;
            println(ins.getAddress() + " " + ins);
            ins = ins.getNext();
        }
    }
}
