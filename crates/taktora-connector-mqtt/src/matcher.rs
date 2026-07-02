//! Local MQTT topic matcher — does a subscription filter `F` match a
//! concrete topic `T`? `REQ_0254`, `REQ_0987` (groundwork for the M2b
//! gateway-local demux, `ADR_0129`).
//!
//! Matching rules (MQTT 3.1.1 §4.7):
//!
//! * A `+` matches exactly one topic level.
//! * A `#` matches the parent level and any number (zero or more) of
//!   trailing levels; it is only legal as the final filter level (enforced
//!   at [`crate::MqttTopicFilter`] construction).
//! * All other levels match literally.

use crate::topic::{MqttTopic, MqttTopicFilter};

/// Return `true` when `filter` matches `topic` under the MQTT wildcard
/// rules.
#[must_use]
pub fn topic_matches(filter: &MqttTopicFilter, topic: &MqttTopic) -> bool {
    let filter_levels: Vec<&str> = filter.levels().collect();
    let topic_levels: Vec<&str> = topic.levels().collect();

    let mut i = 0;
    loop {
        if i == filter_levels.len() {
            // Filter exhausted — a match iff the topic is exhausted too.
            return i == topic_levels.len();
        }
        let f = filter_levels[i];
        if f == "#" {
            // Multi-level wildcard matches the remaining zero-or-more
            // levels (including the parent, so `sport/#` matches `sport`).
            return true;
        }
        if i == topic_levels.len() {
            // Topic exhausted but filter still has a non-`#` level to
            // match (e.g. `sport/+` vs `sport`).
            return false;
        }
        let t = topic_levels[i];
        if f == "+" || f == t {
            i += 1;
            continue;
        }
        return false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn filter(s: &str) -> MqttTopicFilter {
        MqttTopicFilter::new(s).unwrap()
    }
    fn topic(s: &str) -> MqttTopic {
        MqttTopic::new(s).unwrap()
    }

    #[test]
    fn matching_table() {
        // (filter, topic, expected)
        let cases = [
            ("sport/tennis/player1", "sport/tennis/player1", true),
            ("sport/tennis/player1", "sport/tennis/player2", false),
            ("sport/+/player1", "sport/tennis/player1", true),
            ("sport/+/player1", "sport/tennis/coach", false),
            ("sport/+", "sport/tennis", true),
            ("sport/+", "sport", false),
            ("sport/+", "sport/tennis/player1", false),
            ("sport/#", "sport/tennis/player1", true),
            ("sport/#", "sport/tennis", true),
            ("sport/#", "sport", true), // `#` matches the parent level
            ("sport/#", "sportz", false),
            ("#", "a/b/c", true),
            ("#", "a", true),
            ("+", "a", true),
            ("+", "a/b", false),
            ("+/+", "/finance", true), // leading slash => empty first level
            ("/finance", "/finance", true),
            ("/finance", "finance", false),
        ];
        for (f, t, expected) in cases {
            assert_eq!(
                topic_matches(&filter(f), &topic(t)),
                expected,
                "filter {f:?} vs topic {t:?}"
            );
        }
    }

    fn plain_level() -> impl Strategy<Value = String> {
        "[a-z]{1,5}"
    }

    proptest! {
        /// The `#` wildcard alone matches every concrete topic.
        #[test]
        fn hash_matches_everything(levels in proptest::collection::vec(plain_level(), 1..6)) {
            let t = topic(&levels.join("/"));
            prop_assert!(topic_matches(&filter("#"), &t));
        }

        /// A concrete filter (no wildcards) matches exactly its own topic
        /// and nothing longer or shorter.
        #[test]
        fn exact_filter_matches_only_itself(levels in proptest::collection::vec(plain_level(), 1..6)) {
            let s = levels.join("/");
            let f = filter(&s);
            prop_assert!(topic_matches(&f, &topic(&s)));
            // Append a level: no longer a match.
            let longer = format!("{s}/extra");
            prop_assert!(!topic_matches(&f, &topic(&longer)));
        }

        /// `prefix/#` matches `prefix` and any extension of it.
        #[test]
        fn prefix_hash_matches_extensions(
            prefix in proptest::collection::vec(plain_level(), 1..4),
            tail in proptest::collection::vec(plain_level(), 0..4),
        ) {
            let f = filter(&format!("{}/#", prefix.join("/")));
            let mut all = prefix.clone();
            all.extend(tail);
            prop_assert!(topic_matches(&f, &topic(&all.join("/"))));
        }

        /// `prefix/+` matches exactly one extra level, never two.
        #[test]
        fn prefix_plus_matches_one_level(prefix in proptest::collection::vec(plain_level(), 1..4)) {
            let base = prefix.join("/");
            let f = filter(&format!("{base}/+"));
            let one = topic(&format!("{base}/one"));
            let one_two = topic(&format!("{base}/one/two"));
            let bare = topic(&base);
            prop_assert!(topic_matches(&f, &one));
            prop_assert!(!topic_matches(&f, &one_two));
            prop_assert!(!topic_matches(&f, &bare));
        }
    }
}
