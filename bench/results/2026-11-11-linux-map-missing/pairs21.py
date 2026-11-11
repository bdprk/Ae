import subprocess, sys, time, statistics

def run(p):
    t0 = time.perf_counter()
    subprocess.run([p], stdout=subprocess.DEVNULL, check=True)
    return (time.perf_counter() - t0) * 1000

def pair(a, b, n=21):
    ta, tb, rat = [], [], []
    for i in range(n):
        if i % 2 == 0:
            x = run(a); y = run(b)
        else:
            y = run(b); x = run(a)
        ta.append(x); tb.append(y); rat.append(x / y)
    print("%-10s median %7.2f ms" % (a, statistics.median(ta)))
    print("%-10s median %7.2f ms" % (b, statistics.median(tb)))
    print("ratio %s/%s median %.3f min %.3f max %.3f" % (a, b, statistics.median(rat), min(rat), max(rat)))

pair("./miss-old", "./miss-new")
pair("./hit-old", "./hit-new")
pair("./miss-old", "./miss-rs")
