# DB testing notes

## October 20, 2026

users = 4
writes = 100
reads = 250

┌───────────┬──────────┬───────┬───────┬───────┐
│ operation │ avg_time │  p50  │  p95  │  p99  │
│  varchar  │  double  │ int64 │ int64 │ int64 │
├───────────┼──────────┼───────┼───────┼───────┤
│ Put       │   323.16 │    26 │   105 │  2333 │
│ Get       │   34.043 │    23 │    90 │   104 │
└───────────┴──────────┴───────┴───────┴───────┘

## October 22, 2026

Errors:
┌─────────┬──────────┬───────┐
│ status  │ avg_time │ count │
│ varchar │  double  │ int64 │
└─────────┴──────────┴───────┘
            0 rows
Successful requests:
┌───────────┬──────────┬───────┬───────┬───────┐
│ operation │ avg_time │  p50  │  p95  │  p99  │
│  varchar  │  double  │ int64 │ int64 │ int64 │
├───────────┼──────────┼───────┼───────┼───────┤
│ Get       │  55.3473 │    51 │   104 │   136 │
│ Put       │ 57.14025 │    53 │   104 │   139 │
└───────────┴──────────┴───────┴───────┴───────┘

Reads: 10000, writes: 4000, total: 14000
Total time: 197.59 ms
Throughput: 70.85 ops/ms

