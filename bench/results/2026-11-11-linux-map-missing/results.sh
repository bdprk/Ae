#!/bin/bash
# ADR-58 measurement dump: paired old/new probes, controls, sizes, generated C.
cd ~/mapmiss || exit 1
NEW=~/ae-mapmiss-new/target/release/ae
OLD=~/ae-mapmiss-old/target/release/ae

echo "== platform =="
uname -srm
nproc
gcc --version | head -1
rustc --version
echo

echo "== paired probes, 21 alternating reps =="
python3 ~/pairs21.py
echo

echo "== controls, identical binaries, 21 reps =="
cp -f hit-new hit-new-copy
cp -f miss-new miss-new-copy
python3 ~/pair.py ./hit-new ./hit-new-copy
python3 ~/pair.py ./miss-new ./miss-new-copy
echo

echo "== peak memory, timing and sampling in separate passes =="
python3 ~/measure.py ./miss-old
python3 ~/measure.py ./miss-new
python3 ~/measure.py ./hit-old
python3 ~/measure.py ./hit-new
echo

echo "== executable size =="
ls -l miss-old miss-new hit-old hit-new
echo

echo "== generated C size =="
$OLD emit-c ~/miss.ae > miss-old.c
$NEW emit-c ~/miss.ae > miss-new.c
$OLD emit-c ~/hit.ae > hit-old.c
$NEW emit-c ~/hit.ae > hit-new.c
wc -c miss-old.c miss-new.c hit-old.c hit-new.c
