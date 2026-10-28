use ahash::AHashSet;

#[allow(dead_code)]
pub fn jaccard_trigrams(a: &str, b: &str) -> (u64, u64) {
    let ta = trigrams(a);
    let tb = trigrams(b);
    if ta.is_empty() && tb.is_empty() {
        return (1, 1);
    }
    let intersection = ta.intersection(&tb).count();
    let union = ta.union(&tb).count();
    if union == 0 {
        return (1, 1);
    }
    (intersection as u64, union as u64)
}

#[allow(dead_code)]
fn trigrams(s: &str) -> AHashSet<[u8; 3]> {
    let bytes = s.as_bytes();
    if bytes.len() < 3 {
        return AHashSet::new();
    }
    bytes.windows(3).map(|w| [w[0], w[1], w[2]]).collect()
}

#[allow(dead_code)]
pub struct NearDupCache {
    recent: std::collections::VecDeque<String>,
    capacity: usize,
}

#[allow(dead_code)]
impl NearDupCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            recent: std::collections::VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    pub fn is_near_dup(&self, text: &str) -> bool {
        self.recent
            .iter()
            .any(|seen| {
                let (intersection, union) = jaccard_trigrams(seen, text);
                intersection * 100 >= union * 85
            })
    }

    pub fn push(&mut self, text: String) {
        if self.recent.len() == self.capacity {
            self.recent.pop_front();
        }
        self.recent.push_back(text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_strings_score_one() {
        let (intersection, union) = jaccard_trigrams("hello world", "hello world");
        assert_eq!(intersection, union);
    }

    #[test]
    fn completely_different_strings_score_zero() {
        let (intersection, union) = jaccard_trigrams("abcdef", "xyz123");
        assert!(intersection * 100 < union * 10);
    }

    #[test]
    fn near_duplicate_above_threshold() {
        let a = "fn handle_request(req: Request) -> Response { let body = req.body(); body }";
        let b = "fn handle_request(req: Request) -> Response { let body = req.body(); body  }";
        let (intersection, union) = jaccard_trigrams(a, b);
        assert!(intersection * 100 >= union * 85);
    }

    #[test]
    fn near_dup_cache_detects_duplicate() {
        let mut cache = NearDupCache::new(5);
        let text = "fn foo() -> i32 { 42 }".repeat(20);
        cache.push(text.clone());
        let near = format!("{text} ");
        assert!(cache.is_near_dup(&near));
    }

    #[test]
    fn near_dup_cache_passes_unrelated() {
        let mut cache = NearDupCache::new(5);
        cache.push("fn foo() -> i32 { 42 }".repeat(20));
        assert!(!cache.is_near_dup(&"struct Bar { x: u64 }".repeat(20)));
    }

    #[test]
    fn cache_evicts_oldest_when_full() {
        let mut cache = NearDupCache::new(2);
        let old = "aaa bbb ccc ddd eee".repeat(10);
        cache.push(old.clone());
        cache.push("zzz yyy xxx www vvv".repeat(10));
        cache.push("mmm nnn ooo ppp qqq".repeat(10));
        assert!(!cache.is_near_dup(&old));
    }
}
