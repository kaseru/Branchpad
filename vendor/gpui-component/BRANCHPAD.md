# gpui-component 0.4.0 (vá cho Branchpad)

Bản gốc: crate `gpui-component` 0.4.0 trên crates.io (checksum `2f58e192c970…`), dùng qua
`[patch.crates-io]` trong `Cargo.toml` của workspace. Chỉ sửa phần chọn chữ của `TextView`
(mọi chỗ sửa có ghi chú `[Branchpad]`):

- `src/text/text_view.rs`: `selection_slop(ngang, dọc)` cho bắt đầu kéo chọn từ vùng sát ngoài khung chữ;
  bấm đúp / ba lần ghi yêu cầu chọn (`pending_select`) thay vì bắt đầu kéo.
- `src/text/text_view.rs`: `on_select(f)` báo đoạn vừa chọn (sau khi kéo, bấm đúp, bấm ba lần), app dùng để hiện nút trích dẫn.
- `src/text/inline.rs`: đoạn chữ nằm dưới chỗ bấm tự chọn từ (bấm đúp) hoặc cả đoạn (bấm ba lần).

Lên phiên bản gpui-component mới: bỏ patch nếu bản mới đã có bấm đúp chọn từ, không thì chép lại
các chỗ `[Branchpad]`.
