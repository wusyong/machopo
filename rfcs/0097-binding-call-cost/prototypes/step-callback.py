"""One call of a callback of machopo's, stepped an instruction at a time in gdb:
how many instructions it runs, in which functions, and each of them.

    STEP_SYM="'bindings::isolate_state::info_method_trampoline::<...>'" \
    STEP_OUT=out.txt STEP_SKIP=200000 MACHOPO_BENCH_WINDOW=1 \
    MACHOPO_BENCH_SCRIPT="(function () { ...; for (...) { x = n.nodeType; } })(); 0" \
    gdb -q -batch -x step-callback.py --args <the perf build of dom_js's `it`> \
        --ignored --exact bench::bench_calls --test-threads 1

`STEP_SKIP` calls are let through first (the loop's code is optimized by then).
The breakpoint is past the function's prologue (its pushes): count those from
the disassembly.
"""
import gdb, os, collections
sym=os.environ["STEP_SYM"]; out=os.environ["STEP_OUT"]; skip=int(os.environ.get("STEP_SKIP","300000"))
gdb.execute("set pagination off"); gdb.execute("set confirm off")
gdb.execute("handle SIGSEGV nostop noprint pass")
gdb.execute("starti")
t=gdb.Breakpoint(sym)
addr=t.locations[0].address
start=gdb.block_for_pc(addr).function.symtab and gdb.block_for_pc(addr)
while start.function is None: start=start.superblock
entry=start.start
t.delete()
bp=gdb.Breakpoint("*0x%x"%entry)
bp.ignore_count=skip
gdb.execute("continue")
frame=gdb.selected_frame(); sp0=int(gdb.parse_and_eval("$sp"))
lines=[]; per=collections.Counter(); n=0
arch=frame.architecture()
while True:
    pc=int(gdb.parse_and_eval("$pc")); sp=int(gdb.parse_and_eval("$sp"))
    ins=arch.disassemble(pc)[0]["asm"]
    f=gdb.block_for_pc(pc)
    name=None
    try:
        s=gdb.execute("info symbol 0x%x"%pc,to_string=True).strip()
        name=s.split(" in section")[0]
        name=name.rsplit(" + ",1)[0]
    except gdb.error: name="?"
    lines.append((pc,name,ins)); per[name]+=1; n+=1
    if ins.startswith("ret") and sp>=sp0: break
    if n>20000: break
    gdb.execute("stepi",to_string=True)
with open(out,"w") as o:
    o.write("total %d\n"%n)
    for k,v in per.most_common(): o.write("%6d %s\n"%(v,k))
    o.write("\n")
    last=None
    for pc,name,ins in lines:
        if name!=last: o.write("\n== %s\n"%name); last=name
        o.write("  %x  %s\n"%(pc,ins))
gdb.execute("kill")
