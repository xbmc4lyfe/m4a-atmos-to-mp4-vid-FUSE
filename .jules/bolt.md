## 2024-05-29 - Avoid Dynamic Allocation in FUSE Hot Paths
**Learning:** In a FUSE filesystem architecture, methods accessed by `read` or `getattr` are extreme hot paths. Dynamic memory allocations, such as using `format!` or iterator mapping to generate hex strings, cause repeated allocations that degrade performance significantly.
**Action:** When generating strings (like hex encoding hashes) in FUSE hot loops, use pre-allocated Strings (e.g., `String::with_capacity`) combined with lookup tables or bitwise operations instead of dynamic macros like `format!`.
