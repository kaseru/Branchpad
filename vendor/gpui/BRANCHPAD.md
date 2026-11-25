# gpui 0.2.2 (vá cho Branchpad)

Bản gốc: crate `gpui` 0.2.2 trên crates.io (checksum `979b45cfa6ec…`), dùng qua
`[patch.crates-io]` trong `Cargo.toml` của workspace. Đã bỏ `examples/`, `tests/`, `docs/` cùng các mục
`[[example]]` / `[[test]]` trong `Cargo.toml` (không cần để build). Mọi chỗ sửa có ghi chú `[Branchpad]`:

- `src/text_system/line_wrapper.rs`: `is_word_char` tính cả khối Latin Extended Additional
  (`U+1E00–U+1EFF`, phần lớn chữ có dấu tiếng Việt) và dấu kết hợp (`U+0300–U+036F`). Bản gốc thiếu
  nên dòng bị ngắt giữa từ: `đợi` → `đ` / `ợi`.

Lên phiên bản gpui mới: bỏ patch nếu bản mới đã có các khối này, không thì chép lại các chỗ `[Branchpad]`.
