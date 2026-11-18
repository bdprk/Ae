# 24-Hour Differential Fuzz Campaign (M2.13) — 2026-11-18

Revision `6db7808`. Five InstaBox Debian 13 hosts, four workers each; seed space partitioned by host so the
campaign never repeats a program. Every seed is compiled by the MIR block-graph backend, the structured tree
backend, the release profile and the MIR interpreter, and all four outputs must agree byte for byte.

## Launch

```sh
# per host, after: git archive HEAD | ssh <host> 'tar xz -C ~/ae' && cargo build --workspace
cd ~/ae && setsid nohup python3 scripts/diff_fuzz.py --duration 24 --jobs 4 --start <SEED> \
    > ~/ae-fuzz/run.log 2>&1 < /dev/null &
```

| Host | Seed range start | Workers | Log |
|---|---|---|---|
| eva | 0 | 4 | `~/ae-fuzz/run.log` |
| evaluate-1 | 10,000,000 | 4 | `~/ae-fuzz/run.log` |
| evaluate-2 | 20,000,000 | 4 | `~/ae-fuzz/run.log` |
| evaluate-3 | 30,000,000 | 4 | `~/ae-fuzz/run.log` |
| ai | 40,000,000 | 4 | `~/ae-fuzz/run.log` |

Started 2026-11-18 22:29 (+08:00); scheduled stop 2026-11-19 22:29 (+08:00).
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

## Collection

```sh
for h in instabox-node instabox-evaluate-1 instabox-node3 instabox-evaluate-3 instabox-ai; do
  ssh $h 'echo $HOSTNAME; tail -1 ~/ae-fuzz/run.log; grep -c FAILURE ~/ae-fuzz/run.log'
done
```

Certificates expire 24 h after issue (`instabox ssh-cert -ttl 86400 <host>`); renewal is only needed to read the
logs, not for the run to continue.

## Verdict

Pending — the M2.13 gate needs 24 hours of wall clock with zero failures. Any `FAILURE` line names the seed; the
seed's program can be regenerated with `python3 scripts/diff_fuzz.py --seeds 1 --start <seed> --save-corpus DIR`.
