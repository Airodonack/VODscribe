# vodscribe-worker sources

- `main.rs`: the whole worker, run as `vodscribe-worker --model <dir> --provider cpu|cuda --ffmpeg <path> [--batch <n>] [--start <s>] [--end <s>] [--exit-on-stdin-close] <audio>`; with `--exit-on-stdin-close` it stops when the app is gone.
