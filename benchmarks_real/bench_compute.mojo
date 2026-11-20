from std.time import perf_counter

def fib(n: Int) -> Int:
    if n <= 1:
        return n
    return fib(n - 1) + fib(n - 2)

def main():
    var start = perf_counter()
    var res = fib(38)
    var elapsed = perf_counter() - start
    var elapsed_ms = Int(elapsed * 1000.0)
    print("fib(38)=", res, " time_ms=", elapsed_ms)
