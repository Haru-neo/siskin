# Benchmarks

Results from writing programs that do the same work in both Siskin and C.
Siskin was compiled with `--release` and C with `-O2`. Each program was run
several times and the fastest time was recorded.

**Look at the ratios.** Absolute times can vary by as much as 2x depending on the machine and its state at the time.
The meaningful number is the ratio between Siskin and C measured side by side under the same conditions.
The closer to 1.0, the closer to the speed of C.

## Compute speed

| Program | What it does | Siskin / C |
|---|---|---|
| `fib` | 35th Fibonacci number (recursive) | 1.02 – 1.07x |
| `loop` | 100 million additions | 1.00 – 1.03x |
| `sieve` | Count primes up to 1 million | 1.11 – 1.18x |

The difference in `sieve` comes from bounds-checking every array access. C doesn't
do that check. It's the price of out-of-bounds accesses never slipping by silently.

## Allocation speed

2000 "requests", each using 1000 small blocks.
This mirrors a server handling a request and then throwing away all the memory it used at once.

| Approach | Siskin / C | On the same machine |
|---|---|---|
| Arena (`with arena`) | 0.98 – 0.99x | 5.8 ms |
| Allocate and free each block | 0.98 – 1.00x | 22.9 ms |

How to read it:

- Across a row, Siskin and C are the same. Whether with an arena or individual allocations,
  Siskin runs as fast as hand-written C.
- Down the column, the arena is about **4x** faster. There is no per-block free,
  and allocating is nothing more than bumping a pointer forward.
  C shows exactly the same gap. The arena is what's fast,
  not anything special about Siskin.

## Run it yourself

```
siskin build bench/fib.skn -o fib --release && ./fib
cc -O2 bench/fib.c -o fib_c && ./fib_c
```

The C baselines for the memory benchmarks were measured with
`-fno-builtin-malloc -fno-builtin-calloc -fno-builtin-free`, so the compiler can't remove the `malloc`/`free` pairs entirely.
Without these flags C doesn't allocate at all, and the comparison is meaningless.
