# Explicit unspecified pixel-aspect declaration — 2026-10-08

Scope: optional generic output_geometry.assume_square_for_unspecified_sar.
The flag defaults false and allows only unknown, N/A, or 0:1 input tags.
Known non-square inputs remain rejected, including with the flag enabled.
The original source FileRefs/tags are retained; declared setsar normalization
is independently decoded, with picture/audio counts and original timestamps
verified. Receipts distinguish a square-pixel assumption from actual scaling.

Actual targeted fixture session62911: terminal exit0, PASS1, 37.64s.
Covers ES and EN, H264 lossless normalization/compact output, default rejection,
explicit acceptance, original selected hashes, unchanged frames/samples and
known SAR2:1 rejection. First fixture43346 failed on a test-only renamed file
reference; no successful result inferred. Corrected fixture keeps exact names.

Independent bounded static review: /root/dated_chapters_native_delivery_check,
run01a118e5-a981-7140-b23e-bcce87c5536b. RequestedSol/high, actual runtime
unexposed. Reviewed source201befb4... and earlier test372a119b...; no concrete
source blocker. Documentation and fixture naming were corrected subsequently.
No human approval or full-film validation inferred.

Caption known SAR6104943:6110800 and bt470bg-vs-unknown color tags remain outside
this policy; they require separately declared, source-supported correction.
