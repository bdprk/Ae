import subprocess, sys, time, statistics, os

def run(prog):
    t0 = time.perf_counter()
    subprocess.run([prog], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
    return (time.perf_counter() - t0) * 1000

def pair(a, b, reps=11):
    ta, tb, rat = [], [], []
    for i in range(reps):
        if i % 2 == 0:
            x = run(a); y = run(b)
        else:
            y = run(b); x = run(a)
        ta.append(x); tb.append(y); rat.append(x / y)
    return ta, tb, rat

a, b = sys.argv[1], sys.argv[2]
if "/" not in a:
    a = "./" + a
if "/" not in b:
    b = "./" + b
ta, tb, rat = pair(a, b)
print("%-12s median %7.2f ms  raw %s" % (os.path.basename(a), statistics.median(ta), [round(v, 1) for v in ta]))
print("%-12s median %7.2f ms  raw %s" % (os.path.basename(b), statistics.median(tb), [round(v, 1) for v in tb]))
print("paired ratio a/b: median %.3f  raw %s" % (statistics.median(rat), [round(v, 3) for v in rat]))
