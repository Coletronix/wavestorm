//! Reading JSON that may hold blank numbers.
//!
//! A cleared number box leaves NaN in the project, and JSON has no NaN: `serde_json` writes it
//! as `null` and then refuses to read `null` back into an `f64`. [`from_str`] reads such a
//! `null` as NaN wherever a plain number is expected (an `Option<f64>` still reads it as
//! `None`), so a project saved with a blank field opens again with that field blank.

use serde::de::value::{MapDeserializer, SeqDeserializer};
use serde::de::{self, DeserializeOwned, IntoDeserializer, Visitor};
use serde_json::Value;

type Error = serde::de::value::Error;

/// Parses `text` as JSON into `T`, reading `null` as NaN for plain numbers.
pub fn from_str<T: DeserializeOwned>(text: &str) -> Result<T, String> {
    let value: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    from_value(value)
}

/// Deserializes `value` into `T`, reading `null` as NaN for plain numbers.
pub fn from_value<T: DeserializeOwned>(value: Value) -> Result<T, String> {
    T::deserialize(Lenient(value)).map_err(|e| e.to_string())
}

struct Lenient(Value);

impl IntoDeserializer<'_, Error> for Lenient {
    type Deserializer = Self;
    fn into_deserializer(self) -> Self {
        self
    }
}

impl<'de> de::Deserializer<'de> for Lenient {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Error> {
        match self.0 {
            Value::Null => v.visit_unit(),
            Value::Bool(b) => v.visit_bool(b),
            Value::Number(n) => {
                if let Some(i) = n.as_u64() {
                    v.visit_u64(i)
                } else if let Some(i) = n.as_i64() {
                    v.visit_i64(i)
                } else {
                    v.visit_f64(n.as_f64().unwrap_or(f64::NAN))
                }
            }
            Value::String(s) => v.visit_string(s),
            Value::Array(a) => v.visit_seq(SeqDeserializer::new(a.into_iter().map(Lenient))),
            Value::Object(m) => v.visit_map(MapDeserializer::new(m.into_iter().map(|(k, x)| (k, Lenient(x))))),
        }
    }

    fn deserialize_f64<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Error> {
        match self.0 {
            Value::Null => v.visit_f64(f64::NAN),
            other => Lenient(other).deserialize_any(v),
        }
    }

    fn deserialize_f32<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Error> {
        match self.0 {
            Value::Null => v.visit_f32(f32::NAN),
            other => Lenient(other).deserialize_any(v),
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Error> {
        match self.0 {
            Value::Null => v.visit_none(),
            _ => v.visit_some(self),
        }
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(self, _: &'static str, v: V) -> Result<V::Value, Error> {
        v.visit_newtype_struct(self)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _: &'static str,
        _: &'static [&'static str],
        v: V,
    ) -> Result<V::Value, Error> {
        match self.0 {
            Value::String(s) => v.visit_enum(s.into_deserializer()),
            Value::Object(m) if m.len() == 1 => match m.into_iter().next() {
                Some((tag, body)) => v.visit_enum(Variant(tag, Lenient(body))),
                None => Err(de::Error::custom("empty enum")),
            },
            _ => Err(de::Error::custom("expected an enum variant")),
        }
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 char str string
        bytes byte_buf unit unit_struct seq tuple tuple_struct map struct identifier ignored_any
    }
}

struct Variant(String, Lenient);

impl<'de> de::EnumAccess<'de> for Variant {
    type Error = Error;
    type Variant = Lenient;
    fn variant_seed<S: de::DeserializeSeed<'de>>(self, seed: S) -> Result<(S::Value, Lenient), Error> {
        let tag = seed.deserialize(self.0.into_deserializer())?;
        Ok((tag, self.1))
    }
}

impl<'de> de::VariantAccess<'de> for Lenient {
    type Error = Error;
    fn unit_variant(self) -> Result<(), Error> {
        Ok(())
    }
    fn newtype_variant_seed<S: de::DeserializeSeed<'de>>(self, seed: S) -> Result<S::Value, Error> {
        seed.deserialize(self)
    }
    fn tuple_variant<V: Visitor<'de>>(self, _: usize, v: V) -> Result<V::Value, Error> {
        de::Deserializer::deserialize_any(self, v)
    }
    fn struct_variant<V: Visitor<'de>>(self, _: &'static [&'static str], v: V) -> Result<V::Value, Error> {
        de::Deserializer::deserialize_any(self, v)
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    enum Shape {
        Round,
        Box { w: f64, h: f32 },
        Pair(f64, f64),
        One(f64),
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct S {
        a: f64,
        b: Option<f64>,
        c: Vec<f64>,
        n: usize,
        s: String,
        shapes: Vec<Shape>,
        #[serde(default)]
        d: f64,
    }

    #[test]
    fn null_reads_as_nan_for_plain_numbers_only() {
        let s = S {
            a: f64::NAN,
            b: None,
            c: vec![1.0, f64::NAN],
            n: 3,
            s: "x".into(),
            shapes: vec![
                Shape::Round,
                Shape::Box { w: f64::NAN, h: f32::NAN },
                Shape::Pair(f64::NAN, 2.0),
                Shape::One(f64::NAN),
            ],
            d: 0.0,
        };
        let text = serde_json::to_string(&s).unwrap();
        assert!(serde_json::from_str::<S>(&text).is_err());
        let back: S = super::from_str(&text).unwrap();
        assert!(back.a.is_nan() && back.b.is_none() && back.c[0] == 1.0 && back.c[1].is_nan());
        assert_eq!((back.n, back.s.as_str()), (3, "x"));
        assert_eq!(back.shapes[0], Shape::Round);
        assert!(matches!(back.shapes[1], Shape::Box { w, h } if w.is_nan() && h.is_nan()));
        assert!(matches!(back.shapes[2], Shape::Pair(a, b) if a.is_nan() && b == 2.0));
        assert!(matches!(back.shapes[3], Shape::One(a) if a.is_nan()));
        let back: S = super::from_str(r#"{"a":1,"b":2.5,"c":[],"n":0,"s":"","shapes":[]}"#).unwrap();
        assert_eq!((back.a, back.b, back.d), (1.0, Some(2.5), 0.0));
        assert!(super::from_str::<S>(r#"{"a":"x"}"#).is_err());
        assert!(super::from_str::<S>("{").is_err());
    }
}
