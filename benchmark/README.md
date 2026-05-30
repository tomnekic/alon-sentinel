# Benchmark Overview

Validated at 10,000 HTTP monitors on a single 4 vCPU / 8 GB server.

Healthy workload:

- 453,054 checks completed
- 453,666 checks expected
- 99.86% execution rate
- 0.1% missed checks
- 286.8 MB RAM
- 67.9% CPU

Failure storm:

- 10,000 simultaneous failures
- 10,000 open incidents
- 0 duplicate incidents
- 0 worker errors

## Hardware

- Hetzner CPX32
- 4 vCPU
- 8 GB RAM
- AMD EPYC Genoa
- Ubuntu
- Docker
- PostgreSQL 16

## Methodology

All monitors targeted a lightweight HTTP endpoint.

Each benchmark was executed for an extended measurement window.

CPU and RAM were sampled throughout the run.

Expected checks are calculated from:

```text
monitors x benchmark duration / interval
```

Missed checks are reported exactly as measured.

Benchmark windows include startup and shutdown phases which may contribute to a
small number of missed scheduled checks.

## Scale Benchmarks

All scale benchmarks used healthy HTTP targets.

| Monitors | Interval | Missed | RAM | CPU |
| ---: | ---: | ---: | ---: | ---: |
| 500 | 60s | 0 | 240 MB | 3% |
| 1000 | 60s | 116 | 255 MB | 5.9% |
| 5000 | 60s | 26 | 263 MB | 33.6% |
| 10000 | 60s | 612 | 287 MB | 67.9% |

## Failure Storm Benchmark

This benchmark ran 10,000 monitors with all targets failing simultaneously.
All targets intentionally returned failure responses.

| Metric | Result |
| --- | ---: |
| Monitors | 10000 |
| Open incidents | 10000 |
| Duplicate incidents | 0 |
| Worker errors | 0 |
| Checks completed | 452,587 |
| Missed checks | 0.2% |
| RAM | 317 MB |
| CPU | 85.1% |

## Uptime Kuma Comparison

Clean comparison runs were published for 1,000 monitors and 4,000 monitors at a
30 second interval.

The following comparison benchmarks represent selected clean runs. Results
depend on workload, monitor type, interval, database backend, and configuration.

CPU values are Docker container CPU percentages. On a 4 vCPU host, 400%
represents full utilization of all cores.

| Scenario | Tool | Checks completed | Missed checks | Full-stack RAM avg | Full-stack CPU avg |
| --- | --- | ---: | ---: | ---: | ---: |
| 1000 monitors / 30s | Alon/Postgres | 52,000 | 200 (0.4%) | 328.8 MiB | 9.8% |
| 1000 monitors / 30s | Uptime Kuma SQLite | 52,065 | 135 (0.3%) | 1999.4 MiB | 112.7% |
| 1000 monitors / 30s | Uptime Kuma MariaDB | 52,057 | 143 (0.3%) | 1962.1 MiB | 102.9% |
| 4000 monitors / 30s | Alon/Postgres | 208,680 | 0 (0.0%) | 389.8 MiB | 46.2% |
| 4000 monitors / 30s | Uptime Kuma SQLite | 208,875 | 0 (0.0%) | 737.6 MiB | 63.9% |
| 4000 monitors / 30s | Uptime Kuma MariaDB | 208,553 | 113 (0.1%) | 1685.5 MiB | 103.2% |

## Raw Results

- [scale_5000-monitors-ok_20260530_102907.txt](results/scale_5000-monitors-ok_20260530_102907.txt)
- [scale_5000-monitors-ok_20260530_102907.samples.csv](results/scale_5000-monitors-ok_20260530_102907.samples.csv)
- [scale_10000-monitors-ok_20260530_132819.txt](results/scale_10000-monitors-ok_20260530_132819.txt)
- [scale_10000-monitors-ok_20260530_132819.samples.csv](results/scale_10000-monitors-ok_20260530_132819.samples.csv)
- [scale_10000-monitors-fail-storm_20260530_141940.txt](results/scale_10000-monitors-fail-storm_20260530_141940.txt)
- [scale_10000-monitors-fail-storm_20260530_141940.samples.csv](results/scale_10000-monitors-fail-storm_20260530_141940.samples.csv)
- [compare_1000m_20260529_133334.txt](results/compare_1000m_20260529_133334.txt)
- [compare_4000m_20260530_075023.txt](results/compare_4000m_20260530_075023.txt)

## Reproducing Benchmarks

The benchmark harness lives in this directory and uses Docker Compose.

```bash
cd benchmark
docker compose up -d
```

Scale runs are driven by `scripts/scale_bench.sh`. Comparison runs are driven by
`scripts/compare_bench.sh`. Environment configuration is documented in
`.env.example`.

Benchmarks are intended to characterize behavior under a specific workload and
hardware configuration. They are not guarantees of performance in every
deployment.
