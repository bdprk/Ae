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
        # Hash map: keys() returns a sorted copy, so the report order is stable
        # across platforms; the map is released when the program exits.
        ("word_count.ae", b"distinct 8\nbrown 1\ndog 1\nfox 2\njumps 1\nlazy 1\nover 1\nquick 1\nthe 3", True),
        # Character-safe truncation: every cut lands on a UTF-8 boundary, so the
        # ellipsis is never preceded by half a character.
        # Filesystem + clock + ordering in one program: creates its own fixture
        # so the listing and sizes are known, and every step matches on a result.
        ("dir_report.ae", (
            "ready\n"
            "a.txt 6 bytes\ntrue\n"
            "b.txt 4 bytes\ntrue\n"
            "c.txt 8 bytes\ntrue\n"
            "largest 8\n"
            "entries 3"
        ).encode("utf-8"), True),
        # JSON path queries: a member, a nested member, an array element, an
        # escaped \u sequence, a null, a missing name and a type mismatch all
        # come back as values, and the document is read from disk.
        # Generic structs: each argument list is a distinct layout, and the
        # nested string fields must be released exactly once.
        ("pairs.ae", b"id=7\n1\none\n2\nin\nner", True),
        # Generic functions: one C function per argument list, and the owned
        # parameter that is not returned must be released exactly once.
        ("generic_fn.ae", b"41\nada\nleft\ntrue\nwrapped\nk\n2", True),
        # Constraints: one body is emitted for several argument lists, so each
        # instantiation must release its own string parameters exactly once.
        ("constraints.ae", b"5\nae\n9\ntrue\n7\nfalse", True),
        # ADR-40: declarations audited at the front end still emit the same
        # per-instantiation bodies; mixed i64/f64/string/bool arguments must
        # release owned strings exactly once.
        ("generic_constraints.ae", b"5\nae\n4.0\n10\n3.5\nfalse\ntrue", True),
        # Option propagation: a None returns early, a discarded Some payload
        # (string and vec) is released exactly once on the successful path.
        ("option_propagate.ae", b"found!\nnone\nalways\nnone\n1\nnone\n3\nnone", True),
        # Generic borrow parameters: no owned value is moved into the callee,
        # so the caller's strings and vectors must still be intact afterwards.
        ("generic_borrow.ae", b"6\n3\n3\n60\n3\nAE\n10\nbeta", True),
        # find returns an IntResult: the Err branch allocates an error string
        # that the enum release must give back exactly once.
        ("find_text.ae", b"4\n13\nsubstring not found\n0\n7", True),
        # f64: one canonical text form on every platform, explicit conversions,
        # and a checked division that reports x/0.0 instead of returning inf.
        ("float_math.ae", b"count 8\nmean 5.0\nvariance 4.0\nshare 0.5\nthird 0.3333333333333333\nexact 5\nexact failed: f64 value is not an exact integer", True),
        ("json_report.ae", (
            "ready\n"
            "wrote 175 bytes\n"
            "name gateway\n"
            "workers 4\n"
            "debug true\n"
            "limits.rps 120\n"
            "routes[1] /api/v1\n"
            "routes: 'routes' is an array, not a number\n"
            "routes[9]: path segment [9] is past the end of the array\n"
            "owner ops\u00e9quipe\n"
            "timeout: no member named 'timeout'\n"
            "name: 'name' is a string, not a number\n"
            "limits { \"rps\": 120, \"burst\": 40 }\n"
            "counted 3 routes\n"
            "true\n"
            "cleaned\n"
            "done"
        ).encode("utf-8"), True),
        # Generic enums: Option[string] and Option[i64] are separate layouts,
        # and a moved-in payload is released with the value.
        ("maybe.ae", b"longest line: three\nlongest line: (none)\nfirst number: 42\nfirst number: (none)", True),
        # Two modules and a private helper: only `pub` items are reachable, and
        # the import graph is resolved relative to the entry file.
        ("modules/main.ae", b"a=3\nb=2\nc=1\n3 entries, 6 words", True),
        ("text_preview.ae", (
            "这是一行很长的中文文本，用于演示按字符而…\n"
            "short line\n"
            "😀 emoji 也按一个字符计算，不会被…\n"
            "lines 3\nchars 68\nbytes 167"
        ).encode("utf-8"), True),
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
                extra = [str(LEAKCHECK), "-Wl,--wrap=malloc", "-Wl,--wrap=calloc", "-Wl,--wrap=free", "-Wl,--wrap=realloc"]
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
                    "-Wl,--wrap=malloc", "-Wl,--wrap=calloc", "-Wl,--wrap=free", "-Wl,--wrap=realloc", "-o", binary], check=True)
    return parse_leak(subprocess.run([binary], capture_output=True, cwd=tmp).stderr)

def parse_leak(stdout):
    match = re.search(rb"\[leak\] blocks=(-?\d+) bytes=(-?\d+)", stdout)
    assert match, ("missing leak report", stdout)
    return int(match.group(1)), int(match.group(2))

if __name__ == "__main__":
    run()
