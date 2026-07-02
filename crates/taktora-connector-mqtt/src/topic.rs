//! MQTT topic-name and topic-filter types with validation. `REQ_0251`,
//! `REQ_0254`.
//!
//! Two distinct types encode the two sides of the protocol:
//!
//! * [`MqttTopic`] — a concrete **publish** topic name. No wildcards.
//! * [`MqttTopicFilter`] — a **subscription** filter. Wildcards allowed
//!   under the MQTT position rules: `+` occupies an entire level; `#`
//!   appears only as the final level.
//!
//! Both reject the null character and the space character, forbid the
//! empty string, and cap the UTF-8 byte length at [`MAX_TOPIC_BYTES`].
//! A leading `/` is allowed (it denotes a leading zero-length level).

/// Maximum topic / filter length in UTF-8 bytes. MQTT 3.1.1 encodes topic
/// strings with a 16-bit length prefix, so 65535 is the hard ceiling.
pub const MAX_TOPIC_BYTES: usize = 65_535;

/// The MQTT single-level wildcard character.
pub const SINGLE_LEVEL_WILDCARD: char = '+';
/// The MQTT multi-level wildcard character.
pub const MULTI_LEVEL_WILDCARD: char = '#';
/// The MQTT topic-level separator.
pub const LEVEL_SEPARATOR: char = '/';

/// Failure modes of [`MqttTopic::new`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TopicError {
    /// The topic string was empty.
    #[error("invalid topic: must not be empty")]
    Empty,
    /// The topic exceeds [`MAX_TOPIC_BYTES`].
    #[error("invalid topic: exceeds {MAX_TOPIC_BYTES} bytes")]
    TooLong,
    /// The topic contains a wildcard (`+` or `#`), which is not allowed on
    /// the publish side.
    #[error("invalid topic: publish topics must not contain wildcards ('+' or '#')")]
    ContainsWildcard,
    /// The topic contains the null character.
    #[error("invalid topic: must not contain the null character")]
    ContainsNull,
    /// The topic contains a space character.
    #[error("invalid topic: must not contain a space character")]
    ContainsSpace,
}

/// Failure modes of [`MqttTopicFilter::new`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TopicFilterError {
    /// The filter string was empty.
    #[error("invalid topic filter: must not be empty")]
    Empty,
    /// The filter exceeds [`MAX_TOPIC_BYTES`].
    #[error("invalid topic filter: exceeds {MAX_TOPIC_BYTES} bytes")]
    TooLong,
    /// The filter contains the null character.
    #[error("invalid topic filter: must not contain the null character")]
    ContainsNull,
    /// The filter contains a space character.
    #[error("invalid topic filter: must not contain a space character")]
    ContainsSpace,
    /// A `+` did not occupy an entire level (e.g. `sport/te+/player`).
    #[error("invalid topic filter: '+' must occupy an entire level")]
    SingleLevelWildcardNotAlone,
    /// A `#` did not occupy an entire level (e.g. `sport/tennis#`).
    #[error("invalid topic filter: '#' must occupy an entire level")]
    MultiLevelWildcardNotAlone,
    /// A `#` appeared somewhere other than the final level.
    #[error("invalid topic filter: '#' is only allowed as the final level")]
    MultiLevelWildcardNotLast,
}

/// A validated MQTT **publish** topic name (no wildcards).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MqttTopic(String);

impl MqttTopic {
    /// Validate and construct a publish topic.
    ///
    /// # Errors
    ///
    /// See [`TopicError`].
    pub fn new(topic: impl Into<String>) -> Result<Self, TopicError> {
        let topic = topic.into();
        if topic.is_empty() {
            return Err(TopicError::Empty);
        }
        if topic.len() > MAX_TOPIC_BYTES {
            return Err(TopicError::TooLong);
        }
        for ch in topic.chars() {
            match ch {
                '\0' => return Err(TopicError::ContainsNull),
                ' ' => return Err(TopicError::ContainsSpace),
                SINGLE_LEVEL_WILDCARD | MULTI_LEVEL_WILDCARD => {
                    return Err(TopicError::ContainsWildcard);
                }
                _ => {}
            }
        }
        Ok(Self(topic))
    }

    /// Borrow the validated topic string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A validated MQTT **subscription** topic filter (wildcards allowed).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MqttTopicFilter(String);

impl MqttTopicFilter {
    /// Validate and construct a topic filter.
    ///
    /// # Errors
    ///
    /// See [`TopicFilterError`].
    pub fn new(filter: impl Into<String>) -> Result<Self, TopicFilterError> {
        let filter = filter.into();
        if filter.is_empty() {
            return Err(TopicFilterError::Empty);
        }
        if filter.len() > MAX_TOPIC_BYTES {
            return Err(TopicFilterError::TooLong);
        }
        if filter.contains('\0') {
            return Err(TopicFilterError::ContainsNull);
        }
        if filter.contains(' ') {
            return Err(TopicFilterError::ContainsSpace);
        }
        let levels: Vec<&str> = filter.split(LEVEL_SEPARATOR).collect();
        let last = levels.len() - 1;
        for (i, level) in levels.iter().enumerate() {
            if level.contains(MULTI_LEVEL_WILDCARD) {
                if *level != "#" {
                    return Err(TopicFilterError::MultiLevelWildcardNotAlone);
                }
                if i != last {
                    return Err(TopicFilterError::MultiLevelWildcardNotLast);
                }
            }
            if level.contains(SINGLE_LEVEL_WILDCARD) && *level != "+" {
                return Err(TopicFilterError::SingleLevelWildcardNotAlone);
            }
        }
        Ok(Self(filter))
    }

    /// Borrow the validated filter string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // ---- publish-topic table ----

    #[test]
    fn accepts_plain_and_leading_slash_topics() {
        assert!(MqttTopic::new("taktora/examples/pubsub").is_ok());
        assert!(MqttTopic::new("/leading/slash").is_ok(), "leading '/' allowed");
        assert!(MqttTopic::new("a").is_ok());
    }

    #[test]
    fn rejects_bad_publish_topics() {
        assert_eq!(MqttTopic::new(""), Err(TopicError::Empty));
        assert_eq!(MqttTopic::new("a/+/b"), Err(TopicError::ContainsWildcard));
        assert_eq!(MqttTopic::new("a/#"), Err(TopicError::ContainsWildcard));
        assert_eq!(MqttTopic::new("a\0b"), Err(TopicError::ContainsNull));
        assert_eq!(MqttTopic::new("a b"), Err(TopicError::ContainsSpace));
    }

    // ---- filter table ----

    #[test]
    fn accepts_valid_filters() {
        for f in [
            "sport/tennis/player1",
            "sport/+/player1",
            "sport/#",
            "#",
            "+",
            "+/tennis/#",
            "/finance",
        ] {
            assert!(MqttTopicFilter::new(f).is_ok(), "{f} should be valid");
        }
    }

    #[test]
    fn rejects_bad_filters() {
        assert_eq!(MqttTopicFilter::new(""), Err(TopicFilterError::Empty));
        assert_eq!(
            MqttTopicFilter::new("sport/te+/player"),
            Err(TopicFilterError::SingleLevelWildcardNotAlone)
        );
        assert_eq!(
            MqttTopicFilter::new("sport/tennis#"),
            Err(TopicFilterError::MultiLevelWildcardNotAlone)
        );
        assert_eq!(
            MqttTopicFilter::new("sport/#/player"),
            Err(TopicFilterError::MultiLevelWildcardNotLast)
        );
        assert_eq!(
            MqttTopicFilter::new("a\0b"),
            Err(TopicFilterError::ContainsNull)
        );
        assert_eq!(
            MqttTopicFilter::new("a b"),
            Err(TopicFilterError::ContainsSpace)
        );
    }

    // ---- property tests ----

    /// A level string free of wildcards, null, space and slash.
    fn plain_level() -> impl Strategy<Value = String> {
        "[a-zA-Z0-9_-]{1,8}"
    }

    proptest! {
        /// Any wildcard-free, null/space-free, non-empty topic built from
        /// plain levels is a valid publish topic, and is equally a valid
        /// filter (concrete topics are a subset of filters).
        #[test]
        fn plain_topics_are_valid_publish_and_filter(levels in proptest::collection::vec(plain_level(), 1..6)) {
            let topic = levels.join("/");
            prop_assert!(MqttTopic::new(topic.clone()).is_ok(), "topic {topic:?}");
            prop_assert!(MqttTopicFilter::new(topic.clone()).is_ok(), "filter {topic:?}");
        }

        /// Inserting a wildcard character anywhere makes a publish topic invalid.
        #[test]
        fn wildcards_reject_publish_topics(
            levels in proptest::collection::vec(plain_level(), 1..6),
            wc in prop::sample::select(vec!['+', '#']),
        ) {
            let mut topic = levels.join("/");
            topic.push(wc);
            prop_assert_eq!(MqttTopic::new(topic), Err(TopicError::ContainsWildcard));
        }

        /// A `#` in any non-final level is rejected.
        #[test]
        fn hash_not_last_rejected(
            head in proptest::collection::vec(plain_level(), 1..4),
            tail in proptest::collection::vec(plain_level(), 1..4),
        ) {
            let mut levels = head;
            levels.push("#".to_string());
            levels.extend(tail);
            let filter = levels.join("/");
            prop_assert_eq!(
                MqttTopicFilter::new(filter),
                Err(TopicFilterError::MultiLevelWildcardNotLast)
            );
        }
    }
}
