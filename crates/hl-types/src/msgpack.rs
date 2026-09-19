//! 最小限のcanonical msgpackエンコーダ（Hyperliquidのaction署名対象）。
//!
//! Hyperliquidのactionはmsgpackで符号化し、そのバイト列をハッシュ対象にする。
//! ここではSDKが生成するのと同じ最小表現（fixstr/fixmap/positive fixint等）だけを
//! 使う。浮動小数点は使わない（数量・価格は文字列、整数は整数で渡す）。

/// msgpackへ符号化する値。
///
/// actionの組み立てを単純にするため所有型にしている（借用の寿命を呼び出し側へ
/// 漏らさない）。符号化は即時に行うため、確保の量は問題にしない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Nil,
    Bool(bool),
    /// 正の整数。負値は `Int` を使う。
    UInt(u64),
    Int(i64),
    Str(String),
    Bin(Vec<u8>),
    Array(Vec<Value>),
    /// フィールド順を保持するマップ（順序は呼び出し側が固定する）。
    Map(Vec<(Value, Value)>),
}

impl Value {
    /// 文字列値。
    pub fn str(text: &str) -> Self {
        Self::Str(text.to_string())
    }

    /// 所有文字列から。
    pub fn owned(text: String) -> Self {
        Self::Str(text)
    }

    /// 文字列キーのマップを組み立てる。
    pub fn map(entries: Vec<(&str, Value)>) -> Self {
        Self::Map(
            entries
                .into_iter()
                .map(|(key, value)| (Self::str(key), value))
                .collect(),
        )
    }

    /// msgpackバイト列へ符号化する。
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.encode_into(&mut out);
        out
    }

    fn encode_into(&self, out: &mut Vec<u8>) {
        match self {
            Self::Nil => out.push(0xc0),
            Self::Bool(false) => out.push(0xc2),
            Self::Bool(true) => out.push(0xc3),
            Self::UInt(value) => encode_uint(*value, out),
            Self::Int(value) => encode_int(*value, out),
            Self::Str(text) => encode_str(text, out),
            Self::Bin(bytes) => encode_bin(bytes, out),
            Self::Array(items) => {
                encode_array_len(items.len(), out);
                for item in items {
                    item.encode_into(out);
                }
            }
            Self::Map(entries) => {
                encode_map_len(entries.len(), out);
                for (key, value) in entries {
                    key.encode_into(out);
                    value.encode_into(out);
                }
            }
        }
    }
}

fn encode_uint(value: u64, out: &mut Vec<u8>) {
    if value < 0x80 {
        out.push(value as u8);
    } else if value <= u8::MAX as u64 {
        out.push(0xcc);
        out.push(value as u8);
    } else if value <= u16::MAX as u64 {
        out.push(0xcd);
        out.extend_from_slice(&(value as u16).to_be_bytes());
    } else if value <= u32::MAX as u64 {
        out.push(0xce);
        out.extend_from_slice(&(value as u32).to_be_bytes());
    } else {
        // 公式SDKは 2^32 以上の整数を BigInt へ広げる（`_l1.js` の `adjust()`）。
        // BigIntの正値は `@std/msgpack` が setBigUint64 + 0xcf で符号化する。
        out.push(0xcf);
        out.extend_from_slice(&value.to_be_bytes());
    }
}

fn encode_int(value: i64, out: &mut Vec<u8>) {
    if value >= 0 {
        encode_uint(value as u64, out);
        return;
    }
    if value >= -32 {
        out.push((value as i8) as u8);
    } else if value >= i8::MIN as i64 {
        out.push(0xd0);
        out.push((value as i8) as u8);
    } else if value >= i16::MIN as i64 {
        out.push(0xd1);
        out.extend_from_slice(&(value as i16).to_be_bytes());
    } else if value >= i32::MIN as i64 {
        out.push(0xd2);
        out.extend_from_slice(&(value as i32).to_be_bytes());
    } else {
        out.push(0xd3);
        out.extend_from_slice(&value.to_be_bytes());
    }
}

fn encode_str(text: &str, out: &mut Vec<u8>) {
    let bytes = text.as_bytes();
    let len = bytes.len();
    if len < 32 {
        out.push(0xa0 | len as u8);
    } else if len <= u8::MAX as usize {
        out.push(0xd9);
        out.push(len as u8);
    } else if len <= u16::MAX as usize {
        out.push(0xda);
        out.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        out.push(0xdb);
        out.extend_from_slice(&(len as u32).to_be_bytes());
    }
    out.extend_from_slice(bytes);
}

fn encode_bin(bytes: &[u8], out: &mut Vec<u8>) {
    let len = bytes.len();
    if len <= u8::MAX as usize {
        out.push(0xc4);
        out.push(len as u8);
    } else if len <= u16::MAX as usize {
        out.push(0xc5);
        out.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        out.push(0xc6);
        out.extend_from_slice(&(len as u32).to_be_bytes());
    }
    out.extend_from_slice(bytes);
}

fn encode_array_len(len: usize, out: &mut Vec<u8>) {
    if len < 16 {
        out.push(0x90 | len as u8);
    } else if len <= u16::MAX as usize {
        out.push(0xdc);
        out.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        out.push(0xdd);
        out.extend_from_slice(&(len as u32).to_be_bytes());
    }
}

fn encode_map_len(len: usize, out: &mut Vec<u8>) {
    if len < 16 {
        out.push(0x80 | len as u8);
    } else if len <= u16::MAX as usize {
        out.push(0xde);
        out.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        out.push(0xdf);
        out.extend_from_slice(&(len as u32).to_be_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::Value;

    #[test]
    fn encodes_small_strings_and_integers_minimally() {
        assert_eq!(Value::str("a").encode(), vec![0xa1, b'a']);
        assert_eq!(Value::UInt(1).encode(), vec![0x01]);
        assert_eq!(Value::UInt(127).encode(), vec![0x7f]);
        assert_eq!(Value::UInt(128).encode(), vec![0xcc, 0x80]);
        assert_eq!(Value::UInt(300).encode(), vec![0xcd, 0x01, 0x2c]);
        assert_eq!(Value::Int(-1).encode(), vec![0xff]);
        assert_eq!(Value::Bool(true).encode(), vec![0xc3]);
        assert_eq!(Value::Nil.encode(), vec![0xc0]);
    }

    #[test]
    fn preserves_map_order_and_encodes_fixmap() {
        let value = Value::map(vec![("b", Value::UInt(2)), ("a", Value::str("x"))]);
        assert_eq!(
            value.encode(),
            vec![0x82, 0xa1, b'b', 0x02, 0xa1, b'a', 0xa1, b'x']
        );
    }

    #[test]
    fn encodes_bin_as_bin_family() {
        assert_eq!(
            Value::Bin(vec![1, 2, 3]).encode(),
            vec![0xc4, 0x03, 1, 2, 3]
        );
    }

    #[test]
    fn encodes_large_integers_as_uint64_like_the_sdk() {
        // 2^31 は uint32、2^32 以上は uint64（SDKの adjust() が BigInt 化し、
        // @std/msgpack が setBigUint64 + 0xcf で符号化する）。
        let thirty_two_bits = 4_294_967_296u64;
        assert_eq!(
            Value::UInt(4_294_967_295).encode(),
            [vec![0xce], 4_294_967_295u32.to_be_bytes().to_vec()].concat()
        );
        assert_eq!(
            Value::UInt(thirty_two_bits).encode(),
            [vec![0xcf], thirty_two_bits.to_be_bytes().to_vec()].concat()
        );
    }

    #[test]
    fn encodes_nonzero_nonce_as_uint64() {
        let nonce = 1_758_000_000_000u64;
        assert_eq!(
            Value::UInt(nonce).encode(),
            [vec![0xcf], nonce.to_be_bytes().to_vec()].concat()
        );
    }
}
