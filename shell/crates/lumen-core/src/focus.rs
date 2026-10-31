//! Grid focus with tvOS's rules: moves never wrap, Up and Down keep the column
//! (or take a shorter row's last tile), and there is no remembered column.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    Moved,
    /// Nothing that way: an edge, or an empty grid.
    Blocked,
}

/// Which of `count` items, laid out `columns` to a row, has focus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridFocus {
    count: usize,
    columns: usize,
    index: usize,
}

impl GridFocus {
    /// Focus on the first item. `columns` is at least 1.
    pub fn new(count: usize, columns: usize) -> Self {
        Self {
            count,
            columns: columns.max(1),
            index: 0,
        }
    }

    /// The focused item, or `None` for an empty grid.
    pub fn index(&self) -> Option<usize> {
        (self.count > 0).then_some(self.index)
    }

    pub fn row(&self) -> usize {
        self.index / self.columns
    }

    pub fn column(&self) -> usize {
        self.index % self.columns
    }

    pub fn rows(&self) -> usize {
        self.count.div_ceil(self.columns)
    }

    /// Items in `row`; 0 past the last row.
    fn row_len(&self, row: usize) -> usize {
        let start = row * self.columns;
        self.count.saturating_sub(start).min(self.columns)
    }

    /// Focuses `index`, clamped to the last item.
    pub fn set(&mut self, index: usize) {
        self.index = index.min(self.count.saturating_sub(1));
    }

    pub fn move_focus(&mut self, dir: Dir) -> Move {
        if self.count == 0 {
            return Move::Blocked;
        }
        let (row, col) = (self.row(), self.column());
        let target = match dir {
            Dir::Left => (col > 0).then(|| self.index - 1),
            Dir::Right => (col + 1 < self.row_len(row)).then(|| self.index + 1),
            Dir::Up => row.checked_sub(1).map(|r| self.in_row(r, col)),
            Dir::Down => (row + 1 < self.rows()).then(|| self.in_row(row + 1, col)),
        };
        match target {
            Some(index) => {
                self.index = index;
                Move::Moved
            }
            None => Move::Blocked,
        }
    }

    /// The item at `col` in `row`, or the row's last item if it is shorter.
    fn in_row(&self, row: usize, col: usize) -> usize {
        row * self.columns + col.min(self.row_len(row) - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 12 items in rows of 5: 5, 5 and 2, like the simulator's apps.
    fn grid() -> GridFocus {
        GridFocus::new(12, 5)
    }

    #[test]
    fn starts_on_the_first_item() {
        let f = grid();
        assert_eq!(f.index(), Some(0));
        assert_eq!((f.row(), f.column(), f.rows()), (0, 0, 3));
    }

    #[test]
    fn moves_along_a_row_and_stops_at_the_ends() {
        let mut f = grid();
        assert_eq!(f.move_focus(Dir::Left), Move::Blocked);
        for i in 1..5 {
            assert_eq!(f.move_focus(Dir::Right), Move::Moved);
            assert_eq!(f.index(), Some(i));
        }
        assert_eq!(f.move_focus(Dir::Right), Move::Blocked, "no wrap to row 1");
        assert_eq!(f.index(), Some(4));
    }

    #[test]
    fn down_keeps_the_column_or_takes_a_short_rows_last_item() {
        let mut f = grid();
        f.set(3);
        assert_eq!(f.move_focus(Dir::Down), Move::Moved);
        assert_eq!(f.index(), Some(8));
        assert_eq!(f.move_focus(Dir::Down), Move::Moved);
        assert_eq!(f.index(), Some(11), "row 2 has 2 items");
        assert_eq!(f.move_focus(Dir::Down), Move::Blocked);
    }

    #[test]
    fn up_is_geometric_with_no_remembered_column() {
        let mut f = grid();
        f.set(9);
        f.move_focus(Dir::Down);
        assert_eq!(f.index(), Some(11));
        f.move_focus(Dir::Up);
        assert_eq!(f.index(), Some(6), "the tile above, not where we started");
        f.move_focus(Dir::Up);
        assert_eq!(f.index(), Some(1));
        assert_eq!(f.move_focus(Dir::Up), Move::Blocked);
    }

    #[test]
    fn right_stops_at_the_end_of_a_short_row() {
        let mut f = grid();
        f.set(10);
        assert_eq!(f.move_focus(Dir::Right), Move::Moved);
        assert_eq!(f.move_focus(Dir::Right), Move::Blocked);
        assert_eq!(f.index(), Some(11));
    }

    #[test]
    fn an_empty_grid_blocks_everything() {
        let mut f = GridFocus::new(0, 5);
        assert_eq!(f.index(), None);
        assert_eq!(f.rows(), 0);
        for dir in [Dir::Up, Dir::Down, Dir::Left, Dir::Right] {
            assert_eq!(f.move_focus(dir), Move::Blocked);
        }
        f.set(3);
        assert_eq!(f.index(), None);
    }

    #[test]
    fn a_single_item_cannot_move() {
        let mut f = GridFocus::new(1, 5);
        for dir in [Dir::Up, Dir::Down, Dir::Left, Dir::Right] {
            assert_eq!(f.move_focus(dir), Move::Blocked);
        }
    }

    #[test]
    fn set_clamps_and_zero_columns_means_one() {
        let mut f = grid();
        f.set(99);
        assert_eq!(f.index(), Some(11));
        let mut column = GridFocus::new(3, 0);
        assert_eq!(column.move_focus(Dir::Down), Move::Moved);
        assert_eq!(column.index(), Some(1));
    }
}
