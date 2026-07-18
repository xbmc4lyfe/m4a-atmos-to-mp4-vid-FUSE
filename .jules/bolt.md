## 2024-07-18 - Optimize hex string generation in cache_key
**Learning:** In hot loops like `cache_key` that generate strings byte-by-byte (e.g. formatting a SHA256 hash), chaining iterators with `.map(|byte| format!("{byte:02x}"))` and `.collect()` results in numerous unnecessary dynamic string allocations.
**Action:** Use `String::with_capacity` combined with a pre-defined bitwise lookup table `const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";` to assemble hex strings without allocating per byte or mapping.
