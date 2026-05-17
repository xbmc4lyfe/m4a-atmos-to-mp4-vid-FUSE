## 2024-05-18 - Fast Hex String Generation in Rust
**Learning:** `format!("{byte:02x}")` inside `.map().collect()` causes massive allocation overhead for cryptographic hashes since it allocates a `String` per byte before collecting.
**Action:** Always prefer manual byte-mapping into a pre-allocated vector (`vec![0u8; len * 2]`) or `String::with_capacity` when generating hex strings in hot loops. The `unsafe { String::from_utf8_unchecked }` is safe if input is strictly mapped to ASCII hex chars.
