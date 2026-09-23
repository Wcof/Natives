//! 十进制定点精度（实施方案 §7.2 / 审计 B11）。
//!
//! 份额、净值、单价一律 4 位小数；金额、费用一律 2 位小数；
//! 内部用 i64 定点表示，ROUND_DOWN 舍入，禁用二进制浮点累计成本。
//! 输出为正规化字符串（去掉纯零尾随小数位），便于 UI 与 JSON 直接呈现。

/// 定点数：`raw = 值 × 10^scale`。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Fixed {
    raw: i64,
    scale: u32,
}

/// 精度违规：字符串小数位超出目标 scale 且有非零尾数。
#[derive(Debug, PartialEq, Eq)]
pub struct PrecisionError;

const SCALE_NAV: u32 = 4;
const SCALE_AMOUNT: u32 = 2;

impl Fixed {
    pub fn zero(scale: u32) -> Self {
        Self { raw: 0, scale }
    }

    pub fn scale(&self) -> u32 {
        self.scale
    }

    pub fn raw(&self) -> i64 {
        self.raw
    }

    pub fn nav(raw: i64) -> Self {
        Self {
            raw,
            scale: SCALE_NAV,
        }
    }

    pub fn amount(raw: i64) -> Self {
        Self {
            raw,
            scale: SCALE_AMOUNT,
        }
    }

    pub fn is_zero(&self) -> bool {
        self.raw == 0
    }

    pub fn is_negative(&self) -> bool {
        self.raw < 0
    }

    /// 解析十进制字符串（可带正负号、可选小数部分），超出 scale 的
    /// 非零尾数拒绝（PrecisionError）；纯零尾数截断接受。
    pub fn parse(text: &str, scale: u32) -> Result<Self, PrecisionError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(PrecisionError);
        }
        let bytes = text.as_bytes();
        let mut index = 0usize;
        let mut negative = false;
        if bytes[index] == b'+' || bytes[index] == b'-' {
            negative = bytes[index] == b'-';
            index += 1;
        }
        let mut raw: i64 = 0;
        let mut seen_digit = false;
        let mut seen_dot = false;
        let mut trailing_nonzero = false;
        let mut fraction_digits = 0i32;
        while index < bytes.len() {
            let byte = bytes[index];
            if byte == b'.' {
                if seen_dot {
                    return Err(PrecisionError);
                }
                seen_dot = true;
                index += 1;
                continue;
            }
            if !byte.is_ascii_digit() {
                return Err(PrecisionError);
            }
            seen_digit = true;
            let digit = (byte - b'0') as i64;
            if seen_dot {
                if fraction_digits >= scale as i32 {
                    if digit != 0 {
                        trailing_nonzero = true;
                    }
                } else {
                    raw = raw
                        .checked_mul(10)
                        .and_then(|v| v.checked_add(digit))
                        .ok_or(PrecisionError)?;
                    fraction_digits += 1;
                }
            } else {
                raw = raw
                    .checked_mul(10)
                    .and_then(|v| v.checked_add(digit))
                    .ok_or(PrecisionError)?;
            }
            index += 1;
        }
        if !seen_digit || trailing_nonzero {
            return Err(PrecisionError);
        }
        while fraction_digits < scale as i32 {
            raw *= 10;
            fraction_digits += 1;
        }
        Ok(Self {
            raw: if negative { -raw } else { raw },
            scale,
        })
    }

    /// 定点乘法：结果 scale = a.scale + b.scale，再按目标 scale ROUND_DOWN。
    pub fn checked_mul_to(&self, other: &Self, target_scale: u32) -> Option<Self> {
        let wide = (self.raw as i128) * (other.raw as i128);
        let total = (self.scale + other.scale) as i32;
        let mut value = wide;
        let mut drop = total - target_scale as i32;
        let mut truncated_nonzero = false;
        while drop > 0 {
            if value % 10 != 0 {
                truncated_nonzero = true;
            }
            value /= 10;
            drop -= 1;
        }
        if truncated_nonzero {
            // ROUND_DOWN（向零截断），丢弃的尾数不为零是允许的，但须为非负
            // 语义调用方处理；这里只做向零截断。
        }
        if value > i64::MAX as i128 || value < i64::MIN as i128 {
            return None;
        }
        Some(Self {
            raw: value as i64,
            scale: target_scale,
        })
    }

    pub fn checked_add(&self, other: &Self) -> Option<Self> {
        debug_assert_eq!(self.scale, other.scale, "fixed add scale mismatch");
        if self.scale != other.scale {
            return None;
        }
        Some(Self {
            raw: self.raw.checked_add(other.raw)?,
            scale: self.scale,
        })
    }

    pub fn checked_sub(&self, other: &Self) -> Option<Self> {
        debug_assert_eq!(self.scale, other.scale, "fixed sub scale mismatch");
        if self.scale != other.scale {
            return None;
        }
        Some(Self {
            raw: self.raw.checked_sub(other.raw)?,
            scale: self.scale,
        })
    }

    /// 比例乘法：self × ratio，ratio 以 10^ratio_denom 为分母的定点整数
    /// （例如 ratio_raw=30, ratio_denom=100 表示 0.30），结果 ROUND_DOWN 到
    /// self.scale。
    pub fn checked_ratio(&self, ratio_raw: i64, ratio_denom: i64) -> Option<Self> {
        let wide = (self.raw as i128 * ratio_raw as i128) / ratio_denom as i128;
        if wide > i64::MAX as i128 || wide < i64::MIN as i128 {
            return None;
        }
        Some(Self {
            raw: wide as i64,
            scale: self.scale,
        })
    }

    /// 正规化输出：去除尾随零，但至少保留一位小数当值为非整或按要求
    /// 输出 scale 位。这里输出完整 scale 位再去尾随零（整数则不带小数点）。
    pub fn to_string_value(&self) -> String {
        let scale = self.scale as usize;
        let negative = self.raw < 0;
        let mut digits = self.raw.unsigned_abs().to_string();
        while digits.len() <= scale {
            digits.insert(0, '0');
        }
        let (integer, fraction) = digits.split_at(digits.len() - scale);
        let mut fraction = fraction.to_string();
        while fraction.ends_with('0') {
            fraction.pop();
        }
        let mut out = String::new();
        if negative {
            out.push('-');
        }
        out.push_str(integer);
        if !fraction.is_empty() {
            out.push('.');
            out.push_str(&fraction);
        }
        out
    }
}

/// 份额/净值精度（4 位）。
pub const SCALE_SHARE: u32 = 4;

impl std::fmt::Display for Fixed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_string_value())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_nav_precision() {
        let nav = Fixed::parse("1.2345", 4).unwrap();
        assert_eq!(nav.raw(), 12345);
        assert_eq!(nav.to_string_value(), "1.2345");
    }

    #[test]
    fn parse_rejects_excess_nonzero_fraction() {
        assert_eq!(Fixed::parse("1.23456", 4), Err(PrecisionError));
        // 纯零尾数可截断接受
        let ok = Fixed::parse("1.234500", 4).unwrap();
        assert_eq!(ok.raw(), 12345);
    }

    #[test]
    fn parse_amount_two_scale() {
        let amount = Fixed::parse("161.50", 2).unwrap();
        assert_eq!(amount.raw(), 16150);
        assert_eq!(amount.to_string_value(), "161.5");
    }

    #[test]
    fn mul_nav_by_share_rounds_down() {
        let share = Fixed::parse("150", 4).unwrap();
        let nav = Fixed::parse("1.2345", 4).unwrap();
        let value = share.checked_mul_to(&nav, 2).unwrap();
        // 150 × 1.2345 = 185.175 → 金额 2 位 ROUND_DOWN = 185.17
        assert_eq!(value.raw(), 18517);
        assert_eq!(value.to_string_value(), "185.17");
    }

    #[test]
    fn negative_amount_display() {
        let amount = Fixed::parse("-0.05", 2).unwrap();
        assert_eq!(amount.to_string_value(), "-0.05");
    }

    #[test]
    fn ratio_used_by_penetration_style_calc() {
        let amount = Fixed::parse("100.00", 2).unwrap();
        let reduced = amount.checked_ratio(30, 100).unwrap();
        assert_eq!(reduced.to_string_value(), "30");
    }
}
