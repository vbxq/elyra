#[derive(Debug, Clone, Copy)]
pub struct AelysRange {
    pub start: Option<i128>,
    pub end: Option<i128>,
    pub inclusive: bool,
}

impl AelysRange {
    pub fn new(start: Option<i128>, end: Option<i128>, inclusive: bool) -> Self {
        Self {
            start,
            end,
            inclusive,
        }
    }
}
