// Decompile every function that references a string, plus its callees to a depth.
// Headless, read-only, on the analysed project (no re-analysis):
//   tools/re/query.sh 'get_group_volume' 2
// Args: <exact string> [callee depth, default 1]. Output: Ghidra C on stdout.
// @category KONTRA
import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.data.StringDataInstance;
import ghidra.program.model.listing.Data;
import ghidra.program.model.listing.DataIterator;
import ghidra.program.model.listing.Function;
import ghidra.program.model.symbol.Reference;
import java.util.ArrayDeque;
import java.util.LinkedHashMap;
import java.util.Map;

public class DecompileByString extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        String needle = args[0];
        int depth = args.length > 1 ? Integer.parseInt(args[1]) : 1;
        Map<Function, Integer> seen = new LinkedHashMap<>();
        ArrayDeque<Function> queue = new ArrayDeque<>();
        if (needle.startsWith("@")) {  // "@0x140762610,0x140762af0": decompile these functions
            for (String a : needle.substring(1).split(",")) {
                ghidra.program.model.address.Address addr = toAddr(a.trim());
                Function f = getFunctionAt(addr);
                if (f == null) f = getFunctionContaining(addr);
                if (f == null) f = createFunction(addr, null);
                if (f != null && !seen.containsKey(f)) { seen.put(f, 0); queue.add(f); }
                else if (f == null) println("// no function at " + a);
            }
        }
        // Raw NUL-terminated byte search: Ghidra leaves many literals undefined.
        byte[] want = (needle + "\0").getBytes(java.nio.charset.StandardCharsets.UTF_8);
        ghidra.program.model.mem.Memory mem = currentProgram.getMemory();
        ghidra.program.model.address.Address at = mem.getMinAddress();
        while (!needle.startsWith("@") && at != null && !monitor.isCancelled()) {
            at = mem.findBytes(at, want, null, true, monitor);
            if (at == null) break;
            // Require a NUL before, so "x_get_group_volume" does not match.
            byte before = 0;
            try { before = mem.getByte(at.subtract(1)); } catch (Exception ignored) { }
            if (before == 0) {
                println("// string " + at + " refs " + getReferencesTo(at).length);
                java.util.List<ghidra.program.model.address.Address> targets = new java.util.ArrayList<>();
                targets.add(at);
                // Pointer slots holding the string's address (tables, string_view constants).
                long v = at.getOffset();
                byte[] ptr = new byte[8];
                for (int i = 0; i < 8; i++) ptr[i] = (byte) (v >>> (8 * i));
                ghidra.program.model.address.Address p = mem.getMinAddress();
                while ((p = mem.findBytes(p, ptr, null, true, monitor)) != null) {
                    println("// pointer slot " + p + " refs " + getReferencesTo(p).length);
                    targets.add(p);
                    p = p.add(1);
                }
                // Code that computes the address without a recorded reference (RIP-relative LEA).
                for (ghidra.program.model.listing.Instruction ins : currentProgram.getListing().getInstructions(true)) {
                    if (monitor.isCancelled()) break;
                    if (!ins.getMnemonicString().equals("LEA")) continue;
                    for (Object o : ins.getOpObjects(1)) {
                        if (o instanceof ghidra.program.model.scalar.Scalar) continue;
                    }
                    ghidra.program.model.address.Address[] flows = null;
                    for (ghidra.program.model.pcode.PcodeOp op : ins.getPcode()) {
                        for (ghidra.program.model.pcode.Varnode in : op.getInputs()) {
                            if (in.isConstant() && in.getOffset() == v) {
                                Function f = getFunctionContaining(ins.getAddress());
                                println("// lea at " + ins.getAddress() + " in " + (f == null ? "<no function>" : f.getName()));
                                if (f != null && !seen.containsKey(f)) { seen.put(f, 0); queue.add(f); }
                            }
                        }
                    }
                }
                for (ghidra.program.model.address.Address t : targets) {
                    for (Reference r : getReferencesTo(t)) {
                        Function f = getFunctionContaining(r.getFromAddress());
                        if (f != null && !seen.containsKey(f)) { seen.put(f, 0); queue.add(f); }
                    }
                }
            }
            at = at.add(1);
        }
        while (!queue.isEmpty()) {
            Function f = queue.poll();
            int level = seen.get(f);
            if (level >= depth) continue;
            for (Function c : f.getCalledFunctions(monitor)) {
                if (!seen.containsKey(c) && !c.isThunk() && !c.isExternal()) { seen.put(c, level + 1); queue.add(c); }
            }
        }
        DecompInterface dec = new DecompInterface();
        dec.openProgram(currentProgram);
        for (Map.Entry<Function, Integer> e : seen.entrySet()) {
            Function f = e.getKey();
            DecompileResults res = dec.decompileFunction(f, 60, monitor);
            println("\n// ===== depth " + e.getValue() + " " + f.getName() + " @ " + f.getEntryPoint());
            println(res.decompileCompleted() ? res.getDecompiledFunction().getC() : "// decompile failed: " + res.getErrorMessage());
        }
    }
}
