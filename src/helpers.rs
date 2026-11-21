use crate::types::localized::join_language_tag_key;
use crate::{LanguageTag, LocalizedClaim};

use chrono::{DateTime, TimeZone, Utc};
use serde::de::value::{MapAccessDeserializer, StringDeserializer};
use serde::de::{
    DeserializeOwned, DeserializeSeed, Deserializer, Error, IgnoredAny, MapAccess, Visitor,
};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{from_value, Value};

use std::cmp::PartialEq;
use std::fmt::{Debug, Display, Formatter, Result as FormatterResult};
use std::marker::PhantomData;

/// Returns the serialized name of an algorithm or key type without JSON quoting.
pub(crate) fn serialize_identifier(value: &impl Serialize) -> Result<String, serde_json::Error> {
    match serde_json::to_value(value)? {
        Value::String(name) => Ok(name),
        Value::Number(number) => Ok(number.to_string()),
        Value::Bool(boolean) => Ok(boolean.to_string()),
        Value::Null => Ok(String::new()),
        _ => Err(serde::ser::Error::custom("expected a scalar identifier")),
    }
}

pub(crate) fn deserialize_string_or_vec<'de, T, D>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    T: DeserializeOwned,
    D: Deserializer<'de>,
{
    let value: serde_json::Value = Deserialize::deserialize(deserializer)?;
    match from_value::<Vec<T>>(value.clone()) {
        Ok(val) => Ok(val),
        Err(_) => {
            let single_val: T = from_value(value).map_err(Error::custom)?;
            Ok(vec![single_val])
        }
    }
}

pub(crate) fn deserialize_string_or_vec_opt<'de, T, D>(
    deserializer: D,
) -> Result<Option<Vec<T>>, D::Error>
where
    T: DeserializeOwned,
    D: Deserializer<'de>,
{
    let value: serde_json::Value = Deserialize::deserialize(deserializer)?;
    match from_value::<Option<Vec<T>>>(value.clone()) {
        Ok(val) => Ok(val),
        Err(_) => {
            let single_val: T = from_value(value).map_err(Error::custom)?;
            Ok(Some(vec![single_val]))
        }
    }
}

// Attempt to deserialize the value; if the value is null or an error occurs, return None.
// This is useful when deserializing fields that may mean different things in different
// contexts, and where we would rather ignore the result than fail to deserialize. For example,
// the fields in JWKs are not well defined; extensions could theoretically define their own
// field names that overload field names used by other JWK types.
pub(crate) fn deserialize_option_or_none<'de, T, D>(deserializer: D) -> Result<Option<T>, D::Error>
where
    T: DeserializeOwned,
    D: Deserializer<'de>,
{
    let value: serde_json::Value = Deserialize::deserialize(deserializer)?;
    match from_value::<Option<T>>(value) {
        Ok(val) => Ok(val),
        Err(_) => Ok(None),
    }
}

// Buffer each entry before trying T, so a failed parse can't leave the sequence
// deserializer partway through an entry. Invalid sequence syntax still fails.
#[derive(Deserialize)]
#[serde(untagged)]
enum SequenceEntry<T> {
    Valid(T),
    Invalid(IgnoredAny),
}

/// Deserializes a sequence, skipping entries that cannot be deserialized as `T`.
pub(crate) fn deserialize_vec_skip_invalid<'de, T, D>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Vec::<SequenceEntry<T>>::deserialize(deserializer).map(valid_entries)
}

/// Deserializes an optional sequence, skipping invalid entries and preserving `None`.
pub(crate) fn deserialize_vec_skip_invalid_opt<'de, T, D>(
    deserializer: D,
) -> Result<Option<Vec<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Option::<Vec<SequenceEntry<T>>>::deserialize(deserializer)
        .map(|entries| entries.map(valid_entries))
}

fn valid_entries<T>(entries: Vec<SequenceEntry<T>>) -> Vec<T> {
    entries
        .into_iter()
        .filter_map(|entry| match entry {
            SequenceEntry::Valid(value) => Some(value),
            SequenceEntry::Invalid(_) => None,
        })
        .collect()
}

pub trait DeserializeMapField: Sized {
    fn deserialize_map_field<'de, V>(
        map: &mut V,
        field_name: &'static str,
        language_tag: Option<LanguageTag>,
        field_value: Option<Self>,
    ) -> Result<Self, V::Error>
    where
        V: MapAccess<'de>;
}

impl<T> DeserializeMapField for T
where
    T: DeserializeOwned,
{
    fn deserialize_map_field<'de, V>(
        map: &mut V,
        field_name: &'static str,
        language_tag: Option<LanguageTag>,
        field_value: Option<Self>,
    ) -> Result<Self, V::Error>
    where
        V: MapAccess<'de>,
    {
        if field_value.is_some() {
            return Err(serde::de::Error::duplicate_field(field_name));
        } else if let Some(language_tag) = language_tag {
            return Err(serde::de::Error::custom(format!(
                "unexpected language tag `{language_tag}` for key `{field_name}`"
            )));
        }
        map.next_value().map_err(|err| {
            V::Error::custom(format!(
                "{}: {err}",
                join_language_tag_key(field_name, language_tag.as_ref())
            ))
        })
    }
}

impl<T> DeserializeMapField for LocalizedClaim<T>
where
    T: DeserializeOwned,
{
    fn deserialize_map_field<'de, V>(
        map: &mut V,
        field_name: &'static str,
        language_tag: Option<LanguageTag>,
        field_value: Option<Self>,
    ) -> Result<Self, V::Error>
    where
        V: MapAccess<'de>,
    {
        let mut localized_claim = field_value.unwrap_or_default();
        if localized_claim.contains_key(language_tag.as_ref()) {
            return Err(serde::de::Error::custom(format!(
                "duplicate field `{}`",
                join_language_tag_key(field_name, language_tag.as_ref())
            )));
        }

        let localized_value = map.next_value().map_err(|err| {
            V::Error::custom(format!(
                "{}: {err}",
                join_language_tag_key(field_name, language_tag.as_ref())
            ))
        })?;
        localized_claim.insert(language_tag, localized_value);

        Ok(localized_claim)
    }
}

// Some providers return boolean values as strings. Provide support for
// parsing using stdlib.
#[cfg(feature = "accept-string-booleans")]
pub(crate) mod serde_string_bool {
    use serde::{de, Deserializer};

    use std::fmt;

    pub fn deserialize<'de, D>(deserializer: D) -> Result<bool, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct BooleanLikeVisitor;

        impl<'de> de::Visitor<'de> for BooleanLikeVisitor {
            type Value = bool;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("A boolean-like value")
            }

            fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(v)
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                v.parse().map_err(E::custom)
            }
        }
        deserializer.deserialize_any(BooleanLikeVisitor)
    }
}

/// Serde space-delimited string serializer for an `Option<Vec<String>>`.
///
/// This function serializes a string vector into a single space-delimited string.
/// If `string_vec_opt` is `None`, the function serializes it as `None` (e.g., `null`
/// in the case of JSON serialization).
pub(crate) fn serialize_space_delimited_vec<T, S>(
    vec: &[T],
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    T: AsRef<str>,
    S: Serializer,
{
    let space_delimited = vec
        .iter()
        .map(AsRef::<str>::as_ref)
        .collect::<Vec<_>>()
        .join(" ");

    serializer.serialize_str(&space_delimited)
}

pub(crate) trait FlattenFilter {
    fn should_include(field_name: &str) -> bool;
}

/// Helper container for filtering map keys out of serde(flatten). This is needed because
/// [`crate::StandardClaims`] doesn't have a fixed set of field names due to its support for
/// localized claims. Consequently, serde by default passes all of the claims to the deserializer
/// for `AC` (additional claims), leading to duplicate claims. [`FilteredFlatten`] is used for
/// eliminating the duplicate claims.
#[derive(Serialize)]
pub(crate) struct FilteredFlatten<F, T>
where
    F: FlattenFilter,
    T: DeserializeOwned + Serialize,
{
    // Keep Serde's derived flattening to support maps, structs, and tagged enums.
    #[serde(flatten)]
    inner: Flatten<T>,
    #[serde(skip)]
    _phantom: PhantomData<F>,
}
impl<F, T> From<T> for FilteredFlatten<F, T>
where
    F: FlattenFilter,
    T: DeserializeOwned + Serialize,
{
    fn from(value: T) -> Self {
        Self {
            inner: Flatten { inner: value },
            _phantom: PhantomData,
        }
    }
}
impl<F, T> AsRef<T> for FilteredFlatten<F, T>
where
    F: FlattenFilter,
    T: DeserializeOwned + Serialize,
{
    fn as_ref(&self) -> &T {
        self.inner.as_ref()
    }
}
impl<F, T> AsMut<T> for FilteredFlatten<F, T>
where
    F: FlattenFilter,
    T: DeserializeOwned + Serialize,
{
    fn as_mut(&mut self) -> &mut T {
        self.inner.as_mut()
    }
}
impl<F, T> PartialEq for FilteredFlatten<F, T>
where
    F: FlattenFilter,
    T: DeserializeOwned + PartialEq + Serialize,
{
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}
impl<F, T> Clone for FilteredFlatten<F, T>
where
    F: FlattenFilter,
    T: Clone + DeserializeOwned + Serialize,
{
    fn clone(&self) -> Self {
        Self {
            inner: Flatten {
                inner: self.inner.inner.clone(),
            },
            _phantom: PhantomData,
        }
    }
}
impl<F, T> Debug for FilteredFlatten<F, T>
where
    F: FlattenFilter,
    T: Debug + DeserializeOwned + Serialize,
{
    // Transparent Debug since we don't care about this struct.
    fn fmt(&self, f: &mut Formatter) -> FormatterResult {
        Debug::fmt(&self.inner, f)
    }
}

struct FilteredMapAccess<F, M> {
    inner: M,
    filter: PhantomData<F>,
}

impl<'de, F: FlattenFilter, M: MapAccess<'de>> MapAccess<'de> for FilteredMapAccess<F, M> {
    type Error = M::Error;

    fn next_key_seed<S: DeserializeSeed<'de>>(
        &mut self,
        seed: S,
    ) -> Result<Option<S::Value>, Self::Error> {
        while let Some(key) = self.inner.next_key::<String>()? {
            if F::should_include(&key) {
                return seed.deserialize(StringDeserializer::new(key)).map(Some);
            }
            self.inner.next_value::<IgnoredAny>()?;
        }
        Ok(None)
    }

    fn next_value_seed<S: DeserializeSeed<'de>>(
        &mut self,
        seed: S,
    ) -> Result<S::Value, Self::Error> {
        self.inner.next_value_seed(seed)
    }
}

impl<'de, F, T> Deserialize<'de> for FilteredFlatten<F, T>
where
    F: FlattenFilter,
    T: DeserializeOwned + Serialize,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct MapVisitor<F: FlattenFilter, T: DeserializeOwned + Serialize>(PhantomData<(F, T)>);

        impl<'de, F, T> Visitor<'de> for MapVisitor<F, T>
        where
            F: FlattenFilter,
            T: DeserializeOwned + Serialize,
        {
            type Value = Flatten<T>;

            fn expecting(&self, formatter: &mut Formatter) -> FormatterResult {
                formatter.write_str("map type T")
            }

            fn visit_map<V>(self, map: V) -> Result<Self::Value, V::Error>
            where
                V: MapAccess<'de>,
            {
                Deserialize::deserialize(MapAccessDeserializer::new(FilteredMapAccess {
                    inner: map,
                    filter: PhantomData::<F>,
                }))
            }
        }

        Ok(FilteredFlatten {
            inner: deserializer.deserialize_map(MapVisitor(PhantomData::<(F, T)>))?,
            _phantom: PhantomData,
        })
    }
}

#[derive(Deserialize, Serialize)]
struct Flatten<T>
where
    T: DeserializeOwned + Serialize,
{
    #[serde(flatten, bound = "T: DeserializeOwned + Serialize")]
    inner: T,
}
impl<T> AsRef<T> for Flatten<T>
where
    T: DeserializeOwned + Serialize,
{
    fn as_ref(&self) -> &T {
        &self.inner
    }
}
impl<T> AsMut<T> for Flatten<T>
where
    T: DeserializeOwned + Serialize,
{
    fn as_mut(&mut self) -> &mut T {
        &mut self.inner
    }
}
impl<T> PartialEq for Flatten<T>
where
    T: DeserializeOwned + PartialEq + Serialize,
{
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}
impl<T> Debug for Flatten<T>
where
    T: Debug + DeserializeOwned + Serialize,
{
    // Transparent Debug since we don't care about this struct.
    fn fmt(&self, f: &mut Formatter) -> FormatterResult {
        Debug::fmt(&self.inner, f)
    }
}

pub(crate) fn join_vec<T>(entries: &[T]) -> String
where
    T: AsRef<str>,
{
    entries
        .iter()
        .map(AsRef::as_ref)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Newtype around a bool, optionally supporting string values.
#[derive(Debug, Deserialize, Serialize)]
#[serde(transparent)]
pub(crate) struct Boolean(
    #[cfg_attr(
        feature = "accept-string-booleans",
        serde(deserialize_with = "crate::helpers::serde_string_bool::deserialize")
    )]
    pub bool,
);
impl Boolean {
    pub(crate) fn into_inner(self) -> bool {
        self.0
    }
}
impl Display for Boolean {
    fn fmt(&self, f: &mut Formatter) -> Result<(), std::fmt::Error> {
        Display::fmt(&self.0, f)
    }
}

/// Timestamp as seconds since the unix epoch, or optionally an ISO 8601 string.
#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub(crate) enum Timestamp {
    Seconds(serde_json::Number),
    #[cfg(feature = "accept-rfc3339-timestamps")]
    Rfc3339(String),
}
impl Timestamp {
    // The spec is ambiguous about whether seconds should be expressed as integers, or
    // whether floating-point values are allowed. For compatibility with a wide range of
    // clients, we round down to the nearest second.
    pub(crate) fn from_utc(utc: &DateTime<Utc>) -> Self {
        Timestamp::Seconds(utc.timestamp().into())
    }

    pub(crate) fn to_utc(&self) -> Result<DateTime<Utc>, ()> {
        match self {
            Timestamp::Seconds(seconds) => {
                let (secs, nsecs) = if seconds.is_i64() {
                    (seconds.as_i64().ok_or(())?, 0u32)
                } else {
                    let secs_f64 = seconds.as_f64().ok_or(())?;
                    let secs = secs_f64.floor();
                    (
                        secs as i64,
                        ((secs_f64 - secs) * 1_000_000_000.).floor() as u32,
                    )
                };
                Utc.timestamp_opt(secs, nsecs).single().ok_or(())
            }
            #[cfg(feature = "accept-rfc3339-timestamps")]
            Timestamp::Rfc3339(iso) => {
                let datetime = DateTime::parse_from_rfc3339(iso).map_err(|_| ())?;
                Ok(datetime.into())
            }
        }
    }
}

impl Display for Timestamp {
    fn fmt(&self, f: &mut Formatter) -> Result<(), std::fmt::Error> {
        match self {
            Timestamp::Seconds(seconds) => Display::fmt(seconds, f),
            #[cfg(feature = "accept-rfc3339-timestamps")]
            Timestamp::Rfc3339(iso) => Display::fmt(iso, f),
        }
    }
}

/// Serde adapter for a UTC datetime represented by a [`Timestamp`].
pub(crate) mod serde_timestamp {
    use super::*;

    fn into_utc<E: Error>(timestamp: Timestamp) -> Result<DateTime<Utc>, E> {
        timestamp.to_utc().map_err(|_| {
            E::custom(format!(
                "failed to parse `{timestamp}` as UTC datetime (in seconds)"
            ))
        })
    }

    pub(crate) fn deserialize<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Timestamp::deserialize(deserializer).and_then(into_utc)
    }

    pub(crate) fn serialize<S>(source: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        Timestamp::from_utc(source).serialize(serializer)
    }

    /// Serde adapter for an optional UTC datetime represented by a [`Timestamp`].
    pub(crate) mod option {
        use super::*;

        pub(crate) fn deserialize<'de, D>(
            deserializer: D,
        ) -> Result<Option<DateTime<Utc>>, D::Error>
        where
            D: Deserializer<'de>,
        {
            Option::<Timestamp>::deserialize(deserializer)?
                .map(into_utc)
                .transpose()
        }

        pub(crate) fn serialize<S>(
            source: &Option<DateTime<Utc>>,
            serializer: S,
        ) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            source
                .as_ref()
                .map(Timestamp::from_utc)
                .serialize(serializer)
        }
    }
}

new_type![
    #[derive(Deserialize, Hash, Serialize)]
    pub(crate) Base64UrlEncodedBytes(
        #[serde(with = "serde_base64url_byte_array")]
        Vec<u8>
    )
];

mod serde_base64url_byte_array {
    use crate::core::base64_url_safe_no_pad;

    use base64::prelude::BASE64_URL_SAFE_NO_PAD;
    use base64::Engine;
    use serde::de::Error;
    use serde::{Deserialize, Deserializer, Serializer};
    use serde_json::{from_value, Value};

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value: Value = Deserialize::deserialize(deserializer)?;
        let base64_encoded: String = from_value(value).map_err(D::Error::custom)?;

        base64_url_safe_no_pad()
            .decode(&base64_encoded)
            .map_err(|err| {
                D::Error::custom(format!(
                    "invalid base64url encoding `{base64_encoded}`: {err:?}"
                ))
            })
    }

    pub fn serialize<S>(v: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let base64_encoded = BASE64_URL_SAFE_NO_PAD.encode(v);
        serializer.serialize_str(&base64_encoded)
    }
}
