## 2024-05-20 - Fast Hex Encoding in Rust
**Learning:** `format!("{byte:02x}")` inside iterators over byte slices (such as SHA256 hashes) allocates an intermediate `String` per byte, resulting in over 30 allocations for a simple hash mapping.
**Action:** Use manual character pushing with `String::with_capacity(64)` and bitwise shifts against a predefined hex byte array (`b"0123456789abcdef"`) to achieve a zero-allocation map inside hot loops generating cache keys.
