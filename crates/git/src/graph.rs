//! Xếp đường nhánh cho đồ thị commit.
//!
//! Commit đi vào theo thứ tự con trước cha (`--date-order`). Mỗi "làn" (lane)
//! giữ hash của commit nó đang chờ. Làn không dồn chỗ: làn trống được dùng lại,
//! nên đường đi thẳng giữ nguyên cột — dễ đọc và tính được từng dòng một
//! (tải dần không cần tính lại). Ý tưởng tham khảo karyalabs/bonsai (MIT).

/// Vị trí theo chiều dọc trong một dòng.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    Top,
    Center,
    Bottom,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphLine {
    pub from_lane: usize,
    pub from: Anchor,
    pub to_lane: usize,
    pub to: Anchor,
    /// Chỉ số màu (chia lấy dư theo bảng màu khi vẽ).
    pub color: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphRow {
    /// Làn của chấm commit.
    pub lane: usize,
    pub color: usize,
    pub lines: Vec<GraphLine>,
    /// Số làn cần để vẽ dòng này.
    pub width: usize,
    pub is_merge: bool,
}

#[derive(Clone, Debug)]
struct Lane {
    id: String,
    color: usize,
}

#[derive(Clone, Debug, Default)]
pub struct GraphBuilder {
    lanes: Vec<Option<Lane>>,
    next_color: usize,
}

impl GraphBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    fn new_color(&mut self) -> usize {
        let c = self.next_color;
        self.next_color += 1;
        c
    }

    fn free_slot(&self) -> usize {
        self.lanes
            .iter()
            .position(Option::is_none)
            .unwrap_or(self.lanes.len())
    }

    fn set(&mut self, i: usize, lane: Option<Lane>) {
        if i >= self.lanes.len() {
            self.lanes.resize(i + 1, None);
        }
        self.lanes[i] = lane;
    }

    fn find(&self, id: &str) -> Option<usize> {
        self.lanes
            .iter()
            .position(|l| l.as_ref().is_some_and(|l| l.id == id))
    }

    /// Thêm commit kế tiếp, trả về cách vẽ dòng của nó.
    pub fn push(&mut self, id: &str, parents: &[String]) -> GraphRow {
        let matching: Vec<usize> = self
            .lanes
            .iter()
            .enumerate()
            .filter(|(_, l)| l.as_ref().is_some_and(|l| l.id == id))
            .map(|(i, _)| i)
            .collect();

        let (lane, color) = match matching.first() {
            Some(&first) => (first, self.lanes[first].as_ref().map_or(0, |l| l.color)),
            None => (self.free_slot(), self.new_color()),
        };

        let mut lines = Vec::new();
        for (i, l) in self.lanes.iter().enumerate() {
            let Some(l) = l else { continue };
            if l.id == id {
                lines.push(GraphLine {
                    from_lane: i,
                    from: Anchor::Top,
                    to_lane: lane,
                    to: Anchor::Center,
                    color: l.color,
                });
            } else {
                lines.push(GraphLine {
                    from_lane: i,
                    from: Anchor::Top,
                    to_lane: i,
                    to: Anchor::Bottom,
                    color: l.color,
                });
            }
        }
        for &m in &matching {
            self.lanes[m] = None;
        }

        if let Some(first_parent) = parents.first() {
            // Kiểu Fork: làn cứ chạy thẳng xuống tới dòng của cha rồi mới cong vào chấm đó
            // (kể cả khi cha đã có làn khác đang chờ — nhiều làn cùng chờ một commit).
            self.set(
                lane,
                Some(Lane {
                    id: first_parent.clone(),
                    color,
                }),
            );
            lines.push(GraphLine {
                from_lane: lane,
                from: Anchor::Center,
                to_lane: lane,
                to: Anchor::Bottom,
                color,
            });
            for p in &parents[1..] {
                let (slot, c) = match self.find(p) {
                    Some(j) => (j, self.lanes[j].as_ref().map_or(color, |l| l.color)),
                    None => {
                        let slot = self.free_slot();
                        let c = self.new_color();
                        self.set(
                            slot,
                            Some(Lane {
                                id: p.clone(),
                                color: c,
                            }),
                        );
                        (slot, c)
                    }
                };
                lines.push(GraphLine {
                    from_lane: lane,
                    from: Anchor::Center,
                    to_lane: slot,
                    to: Anchor::Bottom,
                    color: c,
                });
            }
        }

        while matches!(self.lanes.last(), Some(None)) {
            self.lanes.pop();
        }

        let width = lines
            .iter()
            .map(|l| l.from_lane.max(l.to_lane) + 1)
            .chain([lane + 1])
            .max()
            .unwrap_or(1);
        GraphRow {
            lane,
            color,
            lines,
            width,
            is_merge: parents.len() > 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn linear_history_stays_in_lane_zero() {
        let mut g = GraphBuilder::new();
        let a = g.push("c", &s(&["b"]));
        let b = g.push("b", &s(&["a"]));
        let c = g.push("a", &[]);
        assert_eq!((a.lane, b.lane, c.lane), (0, 0, 0));
        assert_eq!(c.lines.len(), 1); // chỉ có đường từ trên xuống chấm
    }

    #[test]
    fn merge_opens_and_closes_lane() {
        let mut g = GraphBuilder::new();
        // m = merge(a, f); f -> a; a gốc
        let m = g.push("m", &s(&["a", "f"]));
        assert!(m.is_merge);
        assert_eq!(m.width, 2);
        let f = g.push("f", &s(&["a"]));
        assert_eq!(f.lane, 1);
        // f chạy thẳng xuống làn 1 tới dòng của "a"
        assert!(f
            .lines
            .iter()
            .any(|l| l.from_lane == 1 && l.to_lane == 1 && l.to == Anchor::Bottom));
        let a = g.push("a", &[]);
        assert_eq!(a.lane, 0);
        // làn 1 cong vào chấm "a" ở dòng này
        assert!(a
            .lines
            .iter()
            .any(|l| l.from_lane == 1 && l.to_lane == 0 && l.to == Anchor::Center));
        assert_eq!(a.width, 2);
    }

    #[test]
    fn two_branch_tips_share_parent() {
        let mut g = GraphBuilder::new();
        let x = g.push("x", &s(&["p"]));
        let y = g.push("y", &s(&["p"]));
        assert_eq!(x.lane, 0);
        assert_eq!(y.lane, 1);
        let p = g.push("p", &[]);
        assert_eq!(p.lane, 0);
        // làn 0 đi thẳng xuống chấm; làn 1 cong vào chấm ở dòng p
        assert_eq!(p.lines.len(), 2);
    }
}
