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

- `src/text/inline.rs`: `char_in_selection` chọn đúng từ điểm bắt đầu tới điểm kết thúc khi kéo qua nhiều dòng
  (bản gốc dùng hình chữ nhật bao nên lấy thừa / thiếu chữ ở dòng đầu, dòng cuối). `on_select` cũng báo chuỗi rỗng khi đoạn chọn mất.
- `src/text/text_view.rs`: `TextView::selection()` trả `TextViewSelection` để đọc đoạn đang chọn ngay lúc cần (chuột phải trích dẫn), không phải chờ `on_select`.


- `src/input/element.rs`: `highlight_lines` gọi highlighter một lần cho cả khối dòng đang thấy (bản gốc gộp từng dòng vào Vec tăng dần, bậc hai theo số dòng, làm cuộn file tốn cả một lõi CPU).
