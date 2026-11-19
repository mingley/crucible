# Frame Compression Thresholds

Payloads smaller than `compress_threshold` (default 128 bytes) or payloads whose
zlib output is not strictly smaller than raw input are sent uncompressed with
`FLAG_COMPRESSED` unset.
