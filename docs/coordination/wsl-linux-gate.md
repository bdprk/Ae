# The Linux gate

Windows runs `scripts/verify_generated_c.py`; it cannot run the sanitizer
variant because AddressSanitizer is a Linux toolchain feature. That is
what WSL2 is for:

```sh
wsl -d Ubuntu -e bash /mnt/d/<path-to-repo>/scripts/wsl-linux-gate.sh
```

What it does:

1. builds with `cargo build --locked`;
2. points `target` at `$CARGO_TARGET_DIR` (default
   `/root/.cache/ae-target`, inside ext4, so compilation does not crawl
   over the 9p mount);
3. runs `scripts/verify_generated_c.py --sanitize` with `AE_CC=gcc`.

Use `AE_CC=clang` only deliberately: on this machine clang's ASan build
segfaults on `borrow.ae`, while gcc passes, and GitHub's Ubuntu runner
uses gcc.

Run this gate before pushing anything that touches codegen, the runtime,
or memory management. It is the same class of check CI performs on
`ubuntu-latest`.
