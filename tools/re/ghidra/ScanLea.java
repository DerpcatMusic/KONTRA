// Find RIP-relative `lea reg,[rip+disp]` (and mov/movss loads) whose target is a given address by raw byte scan.
//   RE_SCRIPT=ScanLea.java tools/re/query.sh 0x144e74d90
// Args: target addresses. Output: `lea at <addr> -> <target>` plus the containing function if one exists.
// @category KONTRA
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.mem.MemoryBlock;
import java.util.*;

public class ScanLea extends GhidraScript {
    @Override
    public void run() throws Exception {
        Set<Long> want = new HashSet<>();
        for (String a : getScriptArgs()) want.add(toAddr(a).getOffset());
        for (MemoryBlock b : currentProgram.getMemory().getBlocks()) {
            if (!b.isExecute()) continue;
            byte[] buf = new byte[(int) b.getSize()];
            b.getBytes(b.getStart(), buf);
            for (int i = 0; i + 7 <= buf.length; i++) {
                // REX.W (48/4c) 8D modrm(mod=00, rm=101) disp32
                int rex = buf[i] & 0xff, op = buf[i + 1] & 0xff, modrm = buf[i + 2] & 0xff;
                if ((rex == 0x48 || rex == 0x4c) && op == 0x8d && (modrm & 0xc7) == 0x05) {
                    int disp = (buf[i + 3] & 0xff) | (buf[i + 4] & 0xff) << 8 | (buf[i + 5] & 0xff) << 16 | (buf[i + 6] & 0xff) << 24;
                    long target = b.getStart().getOffset() + i + 7 + disp;
                    if (want.contains(target)) {
                        Address at = b.getStart().add(i);
                        Function f = getFunctionContaining(at);
                        println("lea at " + at + " -> " + Long.toHexString(target) + " in " + (f == null ? "<none>" : f.getEntryPoint().toString()));
                    }
                }
            }
        }
    }
}
