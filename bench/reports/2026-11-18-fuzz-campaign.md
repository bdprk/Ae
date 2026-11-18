# 24-Hour Differential Fuzz Campaign (M2.13) — 2026-11-18

Revision `6db7808`. Five InstaBox Debian 13 hosts, four workers each; seed space partitioned by host so the
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
