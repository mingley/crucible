# SSTable Fence Index Layout

Each SSTable divides sorted UTF-8 key-value entries into fixed-count data blocks.
Between block `i` (`max_key`) and block `i + 1` (`min_key`), the index stores a
shortened fence key `sep = shortest_separator(max_key, next_min_key)` satisfying:

```
max_key.encode("utf-8") <= sep.encode("utf-8") < next_min_key.encode("utf-8")
```

Point lookups binary-search the fence array (`bisect_left`) in UTF-8 byte order.
Because UTF-8 preserves Unicode scalar value ordering, valid codepoint-level
separator shortening preserves exact UTF-8 byte ordering across multi-byte keys.
