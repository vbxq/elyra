// a bound above i64::MAX is a u64 kept as its bit pattern, so the object stays 8-byte aligned
#[derive(Debug, Clone, Copy)]
pub struct AelysRange {
    pub start: Option<i64>,
    pub end: Option<i64>,
    pub inclusive: bool,
    pub unsigned_start: bool,
    pub unsigned_end: bool,
}

impl AelysRange {
    pub fn new(start: Option<i128>, end: Option<i128>, inclusive: bool) -> Option<Self> {
        let split = |bound: Option<i128>| -> Option<(Option<i64>, bool)> {
            match bound {
                None => Some((None, false)),
                Some(value) => match i64::try_from(value) {
                    Ok(value) => Some((Some(value), false)),
                    Err(_) => u64::try_from(value)
                        .ok()
                        .map(|value| (Some(value as i64), true)),
                },
            }
        };
        let (start, unsigned_start) = split(start)?;
        let (end, unsigned_end) = split(end)?;
        Some(Self {
            start,
            end,
            inclusive,
            unsigned_start,
            unsigned_end,
        })
    }

    pub fn start_value(&self) -> Option<i128> {
        self.start.map(|value| wide(value, self.unsigned_start))
    }

    pub fn end_value(&self) -> Option<i128> {
        self.end.map(|value| wide(value, self.unsigned_end))
    }
}

fn wide(value: i64, unsigned: bool) -> i128 {
    if unsigned {
        i128::from(value as u64)
    } else {
        i128::from(value)
    }
}
