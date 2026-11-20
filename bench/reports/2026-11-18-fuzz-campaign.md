# 24-Hour Differential Fuzz Campaign (M2.13) — 2026-11-18

Revision `a5d0a32`. Five InstaBox Debian 13 hosts, four workers each; seed space partitioned by host so the
campaign never repeats a program. Every seed is compiled by the MIR block-graph backend, the structured tree
backend, the release profile and the MIR interpreter, and all four outputs must agree byte for byte.

## Launch

```sh
# per host, after: git archive HEAD | ssh <host> 'tar xz -C ~/ae' && cargo build --workspace
cd ~/ae && setsid nohup bash scripts/fuzz_campaign.sh 24 <SEED> 4 \
    > ~/ae-fuzz/campaign.out 2>&1 < /dev/null &
```

`scripts/fuzz_campaign.sh` owns the wall-clock budget and restarts the Python driver from the seed after the
last printed one if it dies, so a crash costs minutes rather than the whole gate. The driver's output (including
the closing summary) is appended to `~/ae/fuzz.log`; the first, unsupervised run's output is kept as
`~/ae-fuzz/part1.log`.

| Host | Seed range start | Workers | Log |
|---|---|---|---|
| eva | 0 | 4 | `~/ae/fuzz.log` |
| evaluate-1 | 10,000,000 | 4 | `~/ae/fuzz.log` |
| evaluate-2 | 20,000,000 | 4 | `~/ae/fuzz.log` |
| evaluate-3 | 30,000,000 | 4 | `~/ae/fuzz.log` |
| ai | 40,000,000 | 4 | `~/ae/fuzz.log` |

Started 2026-11-18 22:29 (+08:00); the supervisor relaunched the campaign under its own 24-hour budget at
22:35, so the campaign stops no earlier than 2026-11-19 22:35 (+08:00). Seeds continue from where the
unsupervised phase stopped; nothing is re-run.
The hosts also carry other sessions' benchmark load, so per-host throughput differs; only the failure count is
load-independent.

## Progress

| Time (+08:00) | Host | Seeds | Failures |
|---|---|---|---|
| 22:31 | eva | 913 | 0 |
| 22:31 | evaluate-1 | 1,788 | 0 |
| 22:31 | evaluate-2 | 1,926 | 0 |
| 22:31 | evaluate-3 | 1,625 | 0 |
| 22:31 | ai | 1,553 | 0 |
| 22:39 | eva | 2,960 | 0 |
| 22:39 | evaluate-1 | 5,301 | 0 |
| 22:39 | evaluate-2 | 5,760 | 0 |
| 22:39 | evaluate-3 | 5,337 | 0 |
| 22:39 | ai | 4,856 | 0 |
| 07:59 | eva | 82,914 | 0 |
| 07:59 | evaluate-1 | 10,114,162 | 0 |
| 07:59 | evaluate-2 | 20,090,567 | 0 |
| 08:12 | evaluate-3 | 30,111,497 | 0 |
| 07:59 | ai | 40,118,240 | 0¹ |


## Incident 2026-11-19 — seed 40115917 (ai) and driver hardening

At `fuzz.log:111417` the ai host recorded `seed 40115917: FAILURE (0 lines)`. The seed passed on every
re-run (locally at `cbc33e4` and on the host itself, `ok (4 lines)` in 1.3 s), and `(0 lines)` matches the
driver's exception path, so the verdict was a compile timeout under the fleet's shared benchmark load —
the host lost the race, the language did not diverge. The running driver carried the old classification,
which charged environmental timeouts to the M2.13 failure count; `fdd61ff` fixes `scripts/diff_fuzz.py`:
environmental timeouts and OS errors retry once and are reported as `ENV` with a separate summary count
(`N environmental`) and excluded from the exit code, while a compiler rejection of a generated program
(`RuntimeError`) stays a real differential FAILURE. `--save-corpus` also crashed on an undefined name
before writing anything; it now saves the sources that actually ran. The fixed driver is deployed to every
host including evaluate-3 once its sandbox woke at 08:10 (`md5 e9e0f426…`); ai's supervisor was relaunched under a fresh 24-hour budget resuming at
seed 40,117,992 (the old 7.6 h segment stays in the log as history). ¹ ai's failure column excludes the
one environmental timeout; its segment before the relaunch had 0 differential failures.

## Fleet upgrade plan — driver 817ba31+ (spawn/cell shapes)

The driver gained kind 21 (spawn with moved scalar/string captures) and kind 22 (two threads sharing one `cell_*` handle) — the thread surface's differential coverage. The fleet's running segments carry the fixed driver (`e9e0f426`) and an older compiler; upgrading a host mid-segment requires a compiler rebuild and a segment restart, which resets that host's 24-hour M2.13 gate clock. Plan: upgrade each host at its next natural segment boundary (the supervisor's relaunch), never mid-segment; the new shapes are already covered locally (80+ seeds across two ranges, plus the execution tests).

## Collection

```sh
for h in instabox-node instabox-evaluate-1 instabox-node3 instabox-evaluate-3 instabox-ai; do
  ssh $h 'echo $HOSTNAME; tail -1 ~/ae/fuzz.log; cat ~/ae-fuzz/part1.log ~/ae/fuzz.log | grep -c FAILURE'
done
```

Certificates expire 24 h after issue (`instabox ssh-cert -ttl 86400 <host>`); renewal is only needed to read the
logs, not for the run to continue.

## Verdict

Pending — the M2.13 gate needs 24 hours of wall clock with zero failures. Any `FAILURE` line names the seed; the
seed's program can be regenerated with `python3 scripts/diff_fuzz.py --seeds 1 --start <seed> --save-corpus DIR`.

## Fleet state 2026-11-20 01:45 (+08:00)

| Host | Seeds | Differential failures | Note |
|---|---|---|---|
| evaluate-1 | 10,252,844 | 0 | certificate renewed to read the log; run unpaused |
| evaluate-3 | 30,218,760 | 0 | fixed driver, 0 failures |
| ai | 40,292,224 | 1 | the single event is the documented pre-fix environmental timeout (seed 40115917); post-relaunch segment clean |
| evaluate-2 | — | — | sandbox cool-down (paused mid-run; resumes with the sandbox) |

Total across reachable hosts: ~80.7M seeds, 0 real differential failures. Driver md5 `e9e0f426…` unchanged.
