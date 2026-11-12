# Benchmark report: <date and topic>

- Status: <exploratory / verified>; platforms tested: <list>
- Source revision and working tree state: <commit and any local changes>
- Raw results: <relative links, including unsuccessful or excluded runs>
- Purpose and scope: <question, workload, and what is not measured>

## Environment

- OS, architecture, CPU, memory, power policy: <record known values; mark unknowns>
- Compiler and tool versions and executable hashes: <values>
- Cache state and other relevant environment settings: <values>

## Reproduction

```text
<exact commands and working directory>
```

Describe inputs, their hashes, output validation, build flags, repetitions,
execution order, warmup, and how timing, CPU, memory, and artifact size were
measured. State whether outputs were validated for every timed run or separately.

## Results

Report raw samples in the linked files, sample count, median, and range or
another stated dispersion measure. Include units and unavailable measurements.

## Interpretation and limitations

State what the evidence supports, uncontrolled variables, platform coverage,
and any reasons results are not directly comparable. Do not extrapolate from
one workload or platform to overall language or production performance.
