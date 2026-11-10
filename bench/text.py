import sys

# Text statistics for the exploratory benchmark: byte count, token count and the
# total number of bytes inside tokens. text.split(" ") matches the compiled
# versions, which also split on a single space and let newlines end up inside
# tokens. The generated input is ASCII, so character and byte counts agree.

if len(sys.argv) < 2:
    print("usage: textstats <file>")
    sys.exit(1)

with open(sys.argv[1], "rb") as handle:
    raw = handle.read()
text = raw.decode("utf-8")
tokens = text.split(" ")
inside = sum(len(token) for token in tokens)
print(len(raw))
print(len(tokens))
print(inside)
