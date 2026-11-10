"""Compile AE-generated C independently and exercise success and failure paths.

--sanitize uses AddressSanitizer where the toolchain provides it.
--leak adds an allocator wrapper (scripts/leakcheck.c) linked with --wrap, which
works on MinGW-W64 where libasan is unavailable: every generated program must
end with exactly the allocation count of a trivial program, i.e. only the C
runtime's own retained buffers.
"""
import argparse
import os
import re
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LEAKCHECK = ROOT / "scripts" / "leakcheck.c"
TRIVIAL = "fn main() { print(1); }"

def run():
    parser = argparse.ArgumentParser()
    parser.add_argument("--sanitize", action="store_true")
    parser.add_argument("--leak", action="store_true")
    options = parser.parse_args()
    executable = ROOT / "target" / "debug" / ("ae.exe" if os.name == "nt" else "ae")
    cc = os.environ.get("AE_CC", "gcc")
    cases = [
        ("vectors.ae", b"5\n55\n154\n0", True),
        ("arrays.ae", b"60", True),
        ("borrow.ae", b"60", True),
        ("functions.ae", b"720", True),
        ("control-flow.ae", b"45", True),
        ("strings.ae", b"hello\nhello!\nhi there\n8\n\xe4\xb8\xad\xe6\x96\x87 \xf0\x9f\x98\x80", True),
        ("enums.ae", b"27\n20\n0\ncircle r=3\nrect 4x5\nempty\ntext:hi\nnumber:7\nflag:true\n12\nnumber:9", True),
        ("concat.ae", b"field/tail\n[bcd]\nbcd\nfield\na!c\nb", True),
        # Runs inside the temporary directory: the example creates and then
        # removes its own sandbox, so the working tree stays clean.
        ("file_tidy.ae", b"a_small.txt 5 small\nb_medium.txt 60 small\nc_large.txt 300 large\norganized 3 into small/large\ncleared 6 files\ndone", True),
        # `?` propagation: the error path must release everything the arm before
        # it bound, which the allocation baseline checks.
        ("copy_notes.ae", b"copied text notes\n11\n2\ntrue\ncleared the sandbox\ndone", True),
    ]
    with tempfile.TemporaryDirectory(prefix="ae-c-verify-") as tmp:
        tmp = Path(tmp)
        bad = tmp / "bounds.ae"
        bad.write_text("fn main() { let a = [1, 2]; print(a[2]); }", encoding="utf-8")
        trivial = tmp / "trivial.ae"
        trivial.write_text(TRIVIAL, encoding="utf-8")
        # Baseline: allocations the C runtime itself keeps at exit.
        baseline = 0
        if options.leak:
            baseline = leak_counts(executable, cc, tmp, trivial, options)[0]
            print(f"BASELINE leak blocks={baseline}")
        for name, expected, ok in cases + [(str(bad), b"array index out of bounds", False)]:
            source = Path(name) if name == str(bad) else ROOT / "examples" / name
            generated = subprocess.run([executable, "emit-c", source], check=True, capture_output=True).stdout
            cfile = tmp / "test.c"
            binary = tmp / ("test.exe" if os.name == "nt" else "test")
            cfile.write_bytes(generated)
            flags = ["-std=c11", "-pedantic-errors", "-Werror=return-type", "-O1"]
            if options.sanitize:
                flags += ["-fsanitize=address,undefined", "-fno-omit-frame-pointer", "-fno-sanitize-recover=all"]
            extra = []
            if options.leak:
                extra = [str(LEAKCHECK), "-Wl,--wrap=malloc", "-Wl,--wrap=free", "-Wl,--wrap=realloc"]
            subprocess.run([cc, *flags, cfile, *extra, "-o", binary], check=True)
            result = subprocess.run([binary], capture_output=True, cwd=tmp)
            if ok:
                stdout = result.stdout.replace(b"\r\n", b"\n").strip()
                assert result.returncode == 0 and stdout == expected, (name, result.returncode, result.stdout, result.stderr)
            else:
                assert result.returncode != 0 and expected in result.stderr, (name, result.returncode, result.stderr)
            assert b"runtime error:" not in result.stderr and b"ERROR: AddressSanitizer" not in result.stderr, (name, result.stderr)
            if options.leak:
                blocks, leaked = parse_leak(result.stderr)
                # The C runtime keeps a couple of buffers alive at exit; a real
                # leak adds a block on top of that baseline.
                assert blocks == baseline, (name, "allocation blocks retained", blocks, baseline)
                print(f"  leak blocks={blocks} bytes={leaked}")
            print(f"PASS {source.name}")

def leak_counts(executable, cc, tmp, source, options):
    generated = subprocess.run([executable, "emit-c", source], check=True, capture_output=True).stdout
    cfile = tmp / "leak-probe.c"
    binary = tmp / ("leak-probe.exe" if os.name == "nt" else "leak-probe")
    cfile.write_bytes(generated)
    subprocess.run([cc, "-std=c11", "-pedantic-errors", "-O1", cfile, str(LEAKCHECK),
                    "-Wl,--wrap=malloc", "-Wl,--wrap=free", "-Wl,--wrap=realloc", "-o", binary], check=True)
    return parse_leak(subprocess.run([binary], capture_output=True, cwd=tmp).stderr)

def parse_leak(stdout):
    match = re.search(rb"\[leak\] blocks=(-?\d+) bytes=(-?\d+)", stdout)
    assert match, ("missing leak report", stdout)
    return int(match.group(1)), int(match.group(2))

if __name__ == "__main__":
    run()
