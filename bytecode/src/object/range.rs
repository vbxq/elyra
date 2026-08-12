#[derive(Debug, Clone, Copy)]
pub struct AelysRange {
    pub start: Option<i64>,
    pub end: Option<i64>,
    pub inclusive: bool,
}

impl AelysRange {
    pub fn new(start: Option<i64>, end: Option<i64>, inclusive: bool) -> Self {
        Self {
            start,
            end,
            inclusive,
        }
    }
}
