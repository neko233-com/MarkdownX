//! Workflow starters. They are compiled into the server so an empty intranet
//! install still gives a game team, PM, writer and filmmaker a useful first
//! page; the same files live in `/templates` for desktop users and agents.

pub struct Template {
    pub key: &'static str,
    pub title: &'static str,
    pub body: &'static str,
}

pub const TEMPLATES: &[Template] = &[
    Template { key: "game-design", title: "游戏设计文档 / Game design", body: include_str!("../../templates/game-design.md") },
    Template { key: "level-design", title: "关卡设计 / Level design", body: include_str!("../../templates/level-design.md") },
    Template { key: "narrative", title: "剧情与任务 / Narrative", body: include_str!("../../templates/narrative.md") },
    Template { key: "product-prd", title: "产品需求 / Product PRD", body: include_str!("../../templates/product-prd.md") },
    Template { key: "film-treatment", title: "影视文案 / Film treatment", body: include_str!("../../templates/film-treatment.md") },
    Template { key: "content-article", title: "内容创作 / Article", body: include_str!("../../templates/content-article.md") },
    Template { key: "meeting-notes", title: "会议记录 / Meeting notes", body: include_str!("../../templates/meeting-notes.md") },
];

pub fn get(key: &str) -> Option<&'static Template> {
    TEMPLATES.iter().find(|t| t.key == key)
}

pub fn render(key: &str, title: &str) -> Option<String> {
    get(key).map(|t| t.body.replace("{{title}}", title))
}

pub fn list() -> Vec<serde_json::Value> {
    TEMPLATES.iter().map(|t| serde_json::json!({"key": t.key, "title": t.title})).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_template_has_a_title_placeholder() {
        for t in TEMPLATES {
            let out = render(t.key, "Demo").unwrap();
            assert!(out.starts_with("# Demo"), "{}", t.key);
            assert!(!out.contains("{{title}}"));
        }
    }

    #[test]
    fn unknown_template_is_none() {
        assert!(render("nope", "x").is_none());
    }
}
