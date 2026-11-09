"""Compile AE-generated C independently and exercise success and failure paths."""
import argparse
import os
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

def run():
    parser = argparse.ArgumentParser()
    parser.add_argument("--sanitize", action="store_true")
    options = parser.parse_args()
    executable = ROOT / "target" / "debug" / ("ae.exe" if os.name == "nt" else "ae")
    cc = os.environ.get("AE_CC", "gcc")
    cases = [
        ("vectors.ae", b"5\n55\n154\n0", True),
        ("arrays.ae", b"60", True),
        ("borrow.ae", b"60", True),
        ("functions.ae", b"720", True),
        ("control-flow.ae", b"45", True),
    ]
    with tempfile.TemporaryDirectory(prefix="ae-c-verify-") as tmp:
        tmp = Path(tmp)
        bad = tmp / "bounds.ae"
        bad.write_text("fn main() { let a = [1, 2]; print(a[2]); }", encoding="utf-8")
        for name, expected, ok in cases + [(str(bad), b"array index out of bounds", False)]:
            source = Path(name) if name == str(bad) else ROOT / "examples" / name
            generated = subprocess.run([executable, "emit-c", source], check=True, capture_output=True).stdout
            cfile = tmp / "test.c"
            binary = tmp / ("test.exe" if os.name == "nt" else "test")
            cfile.write_bytes(generated)
            flags = ["-std=c11", "-pedantic-errors", "-Werror=return-type", "-O1"]
            if options.sanitize:
                flags += ["-fsanitize=address,undefined", "-fno-omit-frame-pointer", "-fno-sanitize-recover=all"]
            subprocess.run([cc, *flags, cfile, "-o", binary], check=True)
            result = subprocess.run([binary], capture_output=True)
            if ok:
                stdout = result.stdout.replace(b"\r\n", b"\n").strip()
                assert result.returncode == 0 and stdout == expected, (name, result.returncode, result.stderr)
            else:
                assert result.returncode != 0 and expected in result.stderr, (name, result.returncode, result.stderr)
            assert b"runtime error:" not in result.stderr and b"ERROR: AddressSanitizer" not in result.stderr, (name, result.stderr)
            print(f"PASS {source.name}")

if __name__ == "__main__":
    run()
