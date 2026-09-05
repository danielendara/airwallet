use rand::{rngs::SysRng, TryRng};
use zeroize::Zeroizing;

pub const STORY_LENGTH: usize = 6;

/// Stable IDs are the credential vocabulary.  Their spelling and order must never
/// be changed: old encrypted files derive their key from these exact IDs.
pub const CATALOG: [(&str, &str); 30] = [
    ("acorn", "Acorn"),
    ("anchor", "Anchor"),
    ("apple", "Apple"),
    ("balloon", "Balloon"),
    ("book", "Book"),
    ("bridge", "Bridge"),
    ("candle", "Candle"),
    ("castle", "Castle"),
    ("cat", "Cat"),
    ("cloud", "Cloud"),
    ("compass", "Compass"),
    ("crown", "Crown"),
    ("diamond", "Diamond"),
    ("drum", "Drum"),
    ("feather", "Feather"),
    ("fish", "Fish"),
    ("flower", "Flower"),
    ("fox", "Fox"),
    ("globe", "Globe"),
    ("sun", "Sun"),
    ("hammer", "Hammer"),
    ("hat", "Hat"),
    ("heart", "Heart"),
    ("house", "House"),
    ("key", "Key"),
    ("kite", "Kite"),
    ("lantern", "Lantern"),
    ("leaf", "Leaf"),
    ("lemon", "Lemon"),
    ("map", "Map"),
];

pub fn label(id: &str) -> Option<&'static str> {
    CATALOG
        .iter()
        .find(|(candidate, _)| *candidate == id)
        .map(|(_, label)| *label)
}

fn sample_below(
    upper: usize,
    next_u64: &mut impl FnMut() -> Result<u64, String>,
) -> Result<usize, String> {
    debug_assert!(upper > 0);
    let upper = upper as u64;
    // Reject the short tail so modulo reduction remains exactly uniform.
    let accepted_end = u64::MAX - (u64::MAX % upper);
    loop {
        let value = next_u64()?;
        if value < accepted_end {
            return Ok((value % upper) as usize);
        }
    }
}

fn shuffled_ids(
    next_u64: &mut impl FnMut() -> Result<u64, String>,
) -> Result<Vec<&'static str>, String> {
    let mut ids: Vec<_> = CATALOG.iter().map(|(id, _)| *id).collect();
    for i in (1..ids.len()).rev() {
        let j = sample_below(i + 1, next_u64)?;
        ids.swap(i, j);
    }
    Ok(ids)
}

pub fn generate() -> Result<[&'static str; STORY_LENGTH], String> {
    let mut rng = SysRng;
    let mut next_u64 = || {
        rng.try_next_u64()
            .map_err(|e| format!("random story generation failed: {e}"))
    };
    let ids = shuffled_ids(&mut next_u64)?;
    ids[..STORY_LENGTH]
        .try_into()
        .map_err(|_| "could not form a six-object story".to_owned())
}

pub fn shuffled_catalog() -> Result<Vec<&'static str>, String> {
    let mut rng = SysRng;
    let mut next_u64 = || {
        rng.try_next_u64()
            .map_err(|e| format!("random display shuffle failed: {e}"))
    };
    shuffled_ids(&mut next_u64)
}

pub fn encode(ids: &[&str]) -> Result<Zeroizing<String>, String> {
    if ids.len() != STORY_LENGTH || ids.iter().any(|id| label(id).is_none()) {
        return Err("a Coffer Story must contain six catalog objects".to_owned());
    }
    if ids.iter().enumerate().any(|(i, id)| ids[..i].contains(id)) {
        return Err("a Coffer Story cannot repeat an object".to_owned());
    }
    Ok(Zeroizing::new(format!("coffer-story-v1:{}", ids.join("/"))))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generated_stories_are_six_distinct_catalog_ids() {
        let story = generate().unwrap();
        assert_eq!(story.len(), STORY_LENGTH);
        assert!(story.iter().all(|id| label(id).is_some()));
        assert_eq!(
            story.iter().collect::<std::collections::HashSet<_>>().len(),
            STORY_LENGTH
        );
    }
    #[test]
    fn encoding_is_stable_and_unambiguous() {
        assert_eq!(
            encode(&["acorn", "anchor", "apple", "balloon", "book", "bridge"])
                .unwrap()
                .as_str(),
            "coffer-story-v1:acorn/anchor/apple/balloon/book/bridge"
        );
    }

    #[test]
    fn encode_rejects_wrong_length() {
        assert_eq!(
            encode(&["acorn", "anchor", "apple", "balloon", "book"]).unwrap_err(),
            "a Coffer Story must contain six catalog objects"
        );
        assert_eq!(
            encode(&["acorn", "anchor", "apple", "balloon", "book", "bridge", "candle"])
                .unwrap_err(),
            "a Coffer Story must contain six catalog objects"
        );
    }

    #[test]
    fn encode_rejects_unknown_catalog_ids() {
        assert_eq!(
            encode(&[
                "acorn",
                "anchor",
                "apple",
                "balloon",
                "book",
                "not-a-catalog-object"
            ])
            .unwrap_err(),
            "a Coffer Story must contain six catalog objects"
        );
    }

    #[test]
    fn encode_rejects_duplicate_ids() {
        assert_eq!(
            encode(&["acorn", "anchor", "apple", "balloon", "book", "acorn"]).unwrap_err(),
            "a Coffer Story cannot repeat an object"
        );
    }

    #[test]
    fn deterministic_shuffle_is_a_permutation_without_modulo_bias_tail() {
        let mut values = [u64::MAX, 0, 1, 2, 3, 4, 5].into_iter().cycle();
        let mut next = || Ok(values.next().unwrap());
        let ids = shuffled_ids(&mut next).unwrap();
        assert_eq!(ids.len(), CATALOG.len());
        assert_eq!(
            ids.iter().collect::<std::collections::HashSet<_>>().len(),
            CATALOG.len()
        );
    }
}
