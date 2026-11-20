from std.time import perf_counter

def main():
    var start = perf_counter()
    var v = List[String](capacity=500000)
    for i in range(500000):
        v.append(String("item_") + String(i))
    var total: Int = 0
    for i in range(len(v)):
        total += v[i].byte_length()
    var elapsed = perf_counter() - start
    var elapsed_ms = Int(elapsed * 1000.0)
    print("total_len=", total, " time_ms=", elapsed_ms)
