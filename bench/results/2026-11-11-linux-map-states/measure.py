import subprocess, sys, time, statistics, os

def peak_kb(pid):
    try:
        with open("/proc/%d/status" % pid) as h:
            for line in h:
                if line.startswith("VmHWM:"):
                    return int(line.split()[1])
    except OSError:
        pass
    return None

def measure(prog, runs=7):
    times, peaks = [], []
    for _ in range(runs):
        t0 = time.perf_counter()
        p = subprocess.Popen([prog], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        peak = None
        while p.poll() is None:
            v = peak_kb(p.pid)
            if v and (peak is None or v > peak):
                peak = v
            time.sleep(0.001)
        p.wait()
        times.append((time.perf_counter() - t0) * 1000)
        if peak:
            peaks.append(peak)
    return statistics.median(times), max(peaks) if peaks else 0

prog = sys.argv[1]
if "/" not in prog:
    prog = "./" + prog
median, peak = measure(prog)
print("%-14s median %7.2f ms   peak %6.2f MB" % (os.path.basename(prog), median, peak / 1024.0))
