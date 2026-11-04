# KeyVal

This project implements a bit-cask-style key-value store in Rust.
It features a TCP interface and custom load tester to track throughput and latency.

To run the load tester, use the orchestration script: `run/run_load_test.rb`.
You can change database and loat-tester settings using the TOML files in `run/config`.
