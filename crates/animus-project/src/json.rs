//! Pretty-printed, key-order-stable JSON serialization for `Project`.

use crate::error::ProjectError;
use animus_core::doc::Project;

/// Serialize a `Project` to its canonical on-disk JSON text.
///
/// Two-space indent, and `Project`'s use of `IndexMap` (with `serde_json`'s
/// `preserve_order` feature) keeps key order stable across saves, so a
/// re-save of an unmodified project produces no spurious git diff.
///
/// Before stringifying, `project` is walked with [`finite_check::check`] to
/// reject any non-finite (`NaN` or infinite) float.
///
/// This walk runs over the *typed* `Project` via a throwaway
/// [`serde::Serializer`] impl, not over a `serde_json::Value` produced by
/// `serde_json::to_value`. That distinction matters: `serde_json` converts
/// `f32::NAN` / `f32::INFINITY` straight to JSON `null` the moment it is
/// asked to serialize them — `serde_json::to_value` and
/// `serde_json::to_string` both do this silently, with no error. By the
/// time a `NaN` has become a `Value`, it has already become
/// `Value::Null`, indistinguishable from a legitimate `None`, so walking
/// the `Value` tree afterwards cannot recover the fact that a float was
/// bad. Catching it means intercepting `serialize_f32`/`serialize_f64`
/// *before* that conversion happens — otherwise a NaN silently corrupts
/// the file, and the corruption only surfaces later as a baffling type
/// error on some unrelated field when the file is loaded.
pub fn to_json(project: &Project) -> Result<String, ProjectError> {
    finite_check::check(project).map_err(|e| ProjectError::NonFiniteFloat { path: e.0 })?;
    Ok(serde_json::to_string_pretty(project)?)
}

/// A `Serializer` that produces no output at all — it only walks the value
/// tree looking for a non-finite float, recording the field/index path to
/// the first one it finds. See the module-level comment on [`to_json`] for
/// why this has to run over the typed value rather than a `serde_json::Value`.
mod finite_check {
    use serde::Serialize;
    use serde::ser::{
        self, SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant, SerializeTuple,
        SerializeTupleStruct, SerializeTupleVariant,
    };
    use std::fmt;

    #[derive(Debug)]
    pub(super) struct NonFiniteAt(pub(super) String);

    impl fmt::Display for NonFiniteAt {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "non-finite float at {}", self.0)
        }
    }
    impl std::error::Error for NonFiniteAt {}
    impl ser::Error for NonFiniteAt {
        fn custom<T: fmt::Display>(msg: T) -> Self {
            NonFiniteAt(msg.to_string())
        }
    }

    pub(super) fn check<T: Serialize + ?Sized>(value: &T) -> Result<(), NonFiniteAt> {
        value.serialize(Checker {
            path: "$".to_string(),
        })
    }

    #[derive(Clone)]
    struct Checker {
        path: String,
    }

    impl Checker {
        fn field(&self, name: &str) -> Checker {
            Checker {
                path: format!("{}.{}", self.path, name),
            }
        }
        fn index(&self, i: usize) -> Checker {
            Checker {
                path: format!("{}[{}]", self.path, i),
            }
        }
    }

    /// Stringify a map key for the error path. Best-effort: this is only
    /// used to make an error message readable, never to drive control flow.
    fn key_to_string<T: Serialize + ?Sized>(key: &T) -> String {
        serde_json::to_string(key).unwrap_or_else(|_| "?".to_string())
    }

    macro_rules! trivial {
        ($($method:ident : $ty:ty),* $(,)?) => {
            $(
                fn $method(self, _v: $ty) -> Result<(), NonFiniteAt> { Ok(()) }
            )*
        };
    }

    impl ser::Serializer for Checker {
        type Ok = ();
        type Error = NonFiniteAt;
        type SerializeSeq = SeqChecker;
        type SerializeTuple = SeqChecker;
        type SerializeTupleStruct = SeqChecker;
        type SerializeTupleVariant = SeqChecker;
        type SerializeMap = MapChecker;
        type SerializeStruct = StructChecker;
        type SerializeStructVariant = StructChecker;

        trivial!(
            serialize_bool: bool,
            serialize_i8: i8,
            serialize_i16: i16,
            serialize_i32: i32,
            serialize_i64: i64,
            serialize_i128: i128,
            serialize_u8: u8,
            serialize_u16: u16,
            serialize_u32: u32,
            serialize_u64: u64,
            serialize_u128: u128,
            serialize_char: char,
            serialize_str: &str,
            serialize_bytes: &[u8],
        );

        fn serialize_f32(self, v: f32) -> Result<(), NonFiniteAt> {
            if v.is_finite() {
                Ok(())
            } else {
                Err(NonFiniteAt(self.path))
            }
        }
        fn serialize_f64(self, v: f64) -> Result<(), NonFiniteAt> {
            if v.is_finite() {
                Ok(())
            } else {
                Err(NonFiniteAt(self.path))
            }
        }

        fn serialize_none(self) -> Result<(), NonFiniteAt> {
            Ok(())
        }
        fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<(), NonFiniteAt> {
            value.serialize(self)
        }
        fn serialize_unit(self) -> Result<(), NonFiniteAt> {
            Ok(())
        }
        fn serialize_unit_struct(self, _name: &'static str) -> Result<(), NonFiniteAt> {
            Ok(())
        }
        fn serialize_unit_variant(
            self,
            _name: &'static str,
            _variant_index: u32,
            _variant: &'static str,
        ) -> Result<(), NonFiniteAt> {
            Ok(())
        }
        fn serialize_newtype_struct<T: ?Sized + Serialize>(
            self,
            _name: &'static str,
            value: &T,
        ) -> Result<(), NonFiniteAt> {
            value.serialize(self)
        }
        fn serialize_newtype_variant<T: ?Sized + Serialize>(
            self,
            _name: &'static str,
            _variant_index: u32,
            variant: &'static str,
            value: &T,
        ) -> Result<(), NonFiniteAt> {
            value.serialize(self.field(variant))
        }

        fn serialize_seq(self, _len: Option<usize>) -> Result<SeqChecker, NonFiniteAt> {
            Ok(SeqChecker { base: self, idx: 0 })
        }
        fn serialize_tuple(self, len: usize) -> Result<SeqChecker, NonFiniteAt> {
            self.serialize_seq(Some(len))
        }
        fn serialize_tuple_struct(
            self,
            _name: &'static str,
            len: usize,
        ) -> Result<SeqChecker, NonFiniteAt> {
            self.serialize_seq(Some(len))
        }
        fn serialize_tuple_variant(
            self,
            _name: &'static str,
            _variant_index: u32,
            variant: &'static str,
            len: usize,
        ) -> Result<SeqChecker, NonFiniteAt> {
            self.field(variant).serialize_seq(Some(len))
        }
        fn serialize_map(self, _len: Option<usize>) -> Result<MapChecker, NonFiniteAt> {
            Ok(MapChecker {
                base: self,
                key: None,
            })
        }
        fn serialize_struct(
            self,
            _name: &'static str,
            _len: usize,
        ) -> Result<StructChecker, NonFiniteAt> {
            Ok(StructChecker { base: self })
        }
        fn serialize_struct_variant(
            self,
            _name: &'static str,
            _variant_index: u32,
            variant: &'static str,
            _len: usize,
        ) -> Result<StructChecker, NonFiniteAt> {
            Ok(StructChecker {
                base: self.field(variant),
            })
        }
        fn collect_str<T: ?Sized + fmt::Display>(self, _value: &T) -> Result<(), NonFiniteAt> {
            Ok(())
        }
    }

    struct SeqChecker {
        base: Checker,
        idx: usize,
    }
    impl SeqChecker {
        fn step<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), NonFiniteAt> {
            let child = self.base.index(self.idx);
            self.idx += 1;
            value.serialize(child)
        }
    }
    impl SerializeSeq for SeqChecker {
        type Ok = ();
        type Error = NonFiniteAt;
        fn serialize_element<T: ?Sized + Serialize>(
            &mut self,
            value: &T,
        ) -> Result<(), NonFiniteAt> {
            self.step(value)
        }
        fn end(self) -> Result<(), NonFiniteAt> {
            Ok(())
        }
    }
    impl SerializeTuple for SeqChecker {
        type Ok = ();
        type Error = NonFiniteAt;
        fn serialize_element<T: ?Sized + Serialize>(
            &mut self,
            value: &T,
        ) -> Result<(), NonFiniteAt> {
            self.step(value)
        }
        fn end(self) -> Result<(), NonFiniteAt> {
            Ok(())
        }
    }
    impl SerializeTupleStruct for SeqChecker {
        type Ok = ();
        type Error = NonFiniteAt;
        fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), NonFiniteAt> {
            self.step(value)
        }
        fn end(self) -> Result<(), NonFiniteAt> {
            Ok(())
        }
    }
    impl SerializeTupleVariant for SeqChecker {
        type Ok = ();
        type Error = NonFiniteAt;
        fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), NonFiniteAt> {
            self.step(value)
        }
        fn end(self) -> Result<(), NonFiniteAt> {
            Ok(())
        }
    }

    struct MapChecker {
        base: Checker,
        key: Option<String>,
    }
    impl SerializeMap for MapChecker {
        type Ok = ();
        type Error = NonFiniteAt;
        fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), NonFiniteAt> {
            self.key = Some(key_to_string(key));
            Ok(())
        }
        fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), NonFiniteAt> {
            let k = self.key.take().unwrap_or_default();
            value.serialize(self.base.field(&k))
        }
        fn end(self) -> Result<(), NonFiniteAt> {
            Ok(())
        }
    }

    struct StructChecker {
        base: Checker,
    }
    impl SerializeStruct for StructChecker {
        type Ok = ();
        type Error = NonFiniteAt;
        fn serialize_field<T: ?Sized + Serialize>(
            &mut self,
            key: &'static str,
            value: &T,
        ) -> Result<(), NonFiniteAt> {
            value.serialize(self.base.field(key))
        }
        fn end(self) -> Result<(), NonFiniteAt> {
            Ok(())
        }
    }
    impl SerializeStructVariant for StructChecker {
        type Ok = ();
        type Error = NonFiniteAt;
        fn serialize_field<T: ?Sized + Serialize>(
            &mut self,
            key: &'static str,
            value: &T,
        ) -> Result<(), NonFiniteAt> {
            value.serialize(self.base.field(key))
        }
        fn end(self) -> Result<(), NonFiniteAt> {
            Ok(())
        }
    }
}
