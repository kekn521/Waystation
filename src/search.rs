use crate::app::Action;
#[derive(Clone, Debug)]
pub struct SearchItem {
    pub id: String,
    pub label: String,
    pub detail: String,
    pub action: Action,
}
pub fn rank(query: &str, items: &[SearchItem]) -> Vec<usize> {
    let q = query.to_lowercase();
    let mut found = vec![];
    for (i, item) in items.iter().enumerate() {
        let label = item.label.to_lowercase();
        let hay = format!("{label} {}", item.detail.to_lowercase());
        let mut chars = hay.chars();
        let matched = q.chars().all(|c| chars.by_ref().any(|h| h == c));
        if matched {
            let score = if label == q {
                0
            } else if label.starts_with(&q) {
                1
            } else if label.contains(&q) {
                2
            } else {
                3
            };
            found.push((score, label.len(), i));
        }
    }
    found.sort_by_key(|&(score, len, i)| (score, len, i));
    found.into_iter().map(|(_, _, i)| i).collect()
}
