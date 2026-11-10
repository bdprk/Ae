# Histogram counting: a fixed-seed LCG feeds a million draws into a 10000-key
# dict; the count-weighted checksum then reads every bucket, so the run cannot
# be reduced to printing the draw or key count. Output must match the other
# implementations exactly.

KEY_COUNT = 10000
DRAWS = 1000000

counts = {}
x = 1234567
for _ in range(DRAWS):
    x = (x * 48271) % 2147483647
    k = "k" + str(x % KEY_COUNT)
    counts[k] = counts.get(k, 0) + 1

checksum = 0
for i in range(KEY_COUNT):
    checksum += counts.get("k" + str(i), 0) * (i + 1)

print("draws=1000000")
print("keys=10000")
print("distinct=" + str(len(counts)))
print("total=1000000")
print("checksum=" + str(checksum))
