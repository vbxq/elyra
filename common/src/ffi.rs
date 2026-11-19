use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FfiType {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
    Bool,
    Ptr,
    CStr,
    Bytes,
    BytesMut,
    Void,
}

impl FfiType {
    pub fn spelling(self) -> &'static str {
        match self {
            FfiType::I8 => "i8",
            FfiType::I16 => "i16",
            FfiType::I32 => "i32",
            FfiType::I64 => "i64",
            FfiType::U8 => "u8",
            FfiType::U16 => "u16",
            FfiType::U32 => "u32",
            FfiType::U64 => "u64",
            FfiType::F32 => "f32",
            FfiType::F64 => "f64",
            FfiType::Bool => "bool",
            FfiType::Ptr => "ptr",
            FfiType::CStr => "cstr",
            FfiType::Bytes => "bytes",
            FfiType::BytesMut => "bytesmut",
            FfiType::Void => "void",
        }
    }

    pub fn from_spelling(text: &str) -> Option<Self> {
        Some(match text {
            "i8" => FfiType::I8,
            "i16" => FfiType::I16,
            "i32" => FfiType::I32,
            "i64" => FfiType::I64,
            "u8" => FfiType::U8,
            "u16" => FfiType::U16,
            "u32" => FfiType::U32,
            "u64" => FfiType::U64,
            "f32" => FfiType::F32,
            "f64" => FfiType::F64,
            "bool" => FfiType::Bool,
            "ptr" => FfiType::Ptr,
            "cstr" => FfiType::CStr,
            "bytes" => FfiType::Bytes,
            "bytesmut" => FfiType::BytesMut,
            "void" => FfiType::Void,
            _ => return None,
        })
    }

    pub fn is_integer(self) -> bool {
        self.int_range().is_some()
    }

    pub fn is_float(self) -> bool {
        matches!(self, FfiType::F32 | FfiType::F64)
    }

    pub fn int_range(self) -> Option<(i128, i128)> {
        Some(match self {
            FfiType::I8 => (i128::from(i8::MIN), i128::from(i8::MAX)),
            FfiType::I16 => (i128::from(i16::MIN), i128::from(i16::MAX)),
            FfiType::I32 => (i128::from(i32::MIN), i128::from(i32::MAX)),
            FfiType::I64 => (i128::from(i64::MIN), i128::from(i64::MAX)),
            FfiType::U8 => (0, i128::from(u8::MAX)),
            FfiType::U16 => (0, i128::from(u16::MAX)),
            FfiType::U32 => (0, i128::from(u32::MAX)),
            FfiType::U64 => (0, i128::from(u64::MAX)),
            _ => return None,
        })
    }

    pub fn is_signed(self) -> bool {
        matches!(
            self,
            FfiType::I8 | FfiType::I16 | FfiType::I32 | FfiType::I64
        )
    }

    pub fn width_bytes(self) -> u8 {
        match self {
            FfiType::I8 | FfiType::U8 | FfiType::Bool => 1,
            FfiType::I16 | FfiType::U16 => 2,
            FfiType::I32 | FfiType::U32 | FfiType::F32 => 4,
            FfiType::Void => 0,
            _ => 8,
        }
    }
}

impl fmt::Display for FfiType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            FfiType::Bytes => "&[u8]",
            FfiType::BytesMut => "&mut [u8]",
            other => other.spelling(),
        };
        f.write_str(text)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FfiSignature {
    pub params: Vec<FfiType>,
    pub fixed: usize,
    pub variadic: bool,
    pub ret: FfiType,
}

impl FfiSignature {
    pub fn encode(&self) -> String {
        let mut text = String::new();
        for (index, param) in self.params.iter().enumerate() {
            if index > 0 {
                text.push(if self.variadic && index == self.fixed {
                    ';'
                } else {
                    ','
                });
            }
            text.push_str(param.spelling());
        }
        if self.variadic && self.fixed == self.params.len() {
            text.push(';');
        }
        if self.variadic && self.fixed == 0 && !self.params.is_empty() {
            text.insert(0, ';');
        }
        text.push('>');
        text.push_str(self.ret.spelling());
        text
    }

    pub fn decode(text: &str) -> Option<Self> {
        let (params_text, ret_text) = text.rsplit_once('>')?;
        let ret = FfiType::from_spelling(ret_text)?;
        let (fixed_text, extra_text) = match params_text.split_once(';') {
            Some((fixed, extra)) => (fixed, Some(extra)),
            None => (params_text, None),
        };
        let parse = |part: &str| -> Option<Vec<FfiType>> {
            if part.is_empty() {
                return Some(Vec::new());
            }
            part.split(',').map(FfiType::from_spelling).collect()
        };
        let mut params = parse(fixed_text)?;
        let fixed = params.len();
        let variadic = extra_text.is_some();
        if let Some(extra) = extra_text {
            params.extend(parse(extra)?);
        }
        if params.contains(&FfiType::Void) {
            return None;
        }
        Some(Self {
            params,
            fixed,
            variadic,
            ret,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures_round_trip() {
        let cases = [
            FfiSignature {
                params: vec![],
                fixed: 0,
                variadic: false,
                ret: FfiType::Void,
            },
            FfiSignature {
                params: vec![FfiType::I32, FfiType::CStr],
                fixed: 2,
                variadic: false,
                ret: FfiType::Ptr,
            },
            FfiSignature {
                params: vec![FfiType::BytesMut, FfiType::U64, FfiType::CStr],
                fixed: 3,
                variadic: true,
                ret: FfiType::I32,
            },
            FfiSignature {
                params: vec![FfiType::CStr, FfiType::I64, FfiType::F64],
                fixed: 1,
                variadic: true,
                ret: FfiType::I32,
            },
            FfiSignature {
                params: vec![FfiType::F64],
                fixed: 0,
                variadic: true,
                ret: FfiType::Void,
            },
        ];
        for case in cases {
            let encoded = case.encode();
            assert_eq!(FfiSignature::decode(&encoded), Some(case), "{encoded}");
        }
    }
}
