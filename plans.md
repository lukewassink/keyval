# Plans

- Checksums
- Buffered reads
  - background thread to flush
- Dedicated threads to handle IO
  - separate read/write channels
  - RW lock to allow simultaneous reads
  - write either when too many writes have built up or too much time has passed
- Optimize memory and cache locality
  - Consider an arena
