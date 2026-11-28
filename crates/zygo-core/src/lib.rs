pub mod client;
pub mod exit;
pub mod hook;
pub mod ids;
pub mod overrides;
pub mod paths;
pub mod session_dir;
pub mod session_view;
pub mod text;
pub mod tokens;
pub mod verbs;

pub use client::{Agent, ClientSpec, Dialect};
pub use text::esc;

#[cfg(test)]
mod tests {
    use super::Agent;

    /// `Agent::index` is an exhaustive match, so a new variant does not compile until it has an index,
    /// and this test fails until the variant is in `Agent::ALL` at that index.
    #[test]
    fn agent_all_holds_each_variant_at_its_index() {
        for (i, a) in Agent::ALL.into_iter().enumerate() {
            assert_eq!(a.index(), i, "{a}");
            assert_eq!(a.as_str().parse::<Agent>().unwrap(), a);
        }
    }

    /// `Agent::COUNT` counts the variants of the one list that declares the enum and `Agent::ALL`,
    /// so `ALL` holds each variant once and cannot miss one.
    #[test]
    fn agent_all_len_is_the_variant_count() {
        assert_eq!(Agent::ALL.len(), Agent::COUNT);
        let mut seen = [false; Agent::COUNT];
        for a in Agent::ALL {
            assert!(!std::mem::replace(&mut seen[a.index()], true), "{a} is in ALL twice");
        }
        assert!(seen.iter().all(|&s| s));
    }
}
