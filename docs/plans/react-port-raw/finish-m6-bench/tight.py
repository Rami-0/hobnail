import sys, time
cg, dur, out = sys.argv[1], float(sys.argv[2]), sys.argv[3]
end = time.time() + dur; best = 0; stat = ""
while time.time() < end:
    try: cur = int(open(cg + "/memory.current").read())
    except OSError: break
    time.sleep(0.002)
    if cur > best:
        best = cur; stat = open(cg + "/memory.stat").read()
open(out, "w").write(f"max {best}\n" + "\n".join(l for l in stat.splitlines() if l.split()[0] in ("anon","file","kernel","kernel_stack","pagetables","sock","shmem","slab","percpu","vmalloc")) + "\n")
