//! UI strings (§4.14): Fluent bundles in `locales/en` and `locales/ru`.

use fluent_templates::fluent_bundle::FluentValue;
use fluent_templates::{LanguageIdentifier, Loader, static_loader};
use std::borrow::Cow;
use std::collections::HashMap;

static_loader! {
    static LOCALES = {
        locales: "../../locales",
        fallback_language: "en",
        customise: |bundle| bundle.set_use_isolating(false),
    };
}

#[derive(Clone, Debug)]
pub struct I18n {
    lang: LanguageIdentifier,
    pub code: &'static str,
}

impl I18n {
    pub fn new(code: &'static str) -> Self {
        let lang: LanguageIdentifier = code.parse().unwrap_or_else(|_| "en".parse().unwrap());
        Self { lang, code }
    }

    pub fn t(&self, key: &str) -> String {
        LOCALES
            .try_lookup(&self.lang, key)
            .unwrap_or_else(|| key.to_string())
    }

    pub fn ta(&self, key: &str, args: &[(&'static str, String)]) -> String {
        let map: HashMap<Cow<'static, str>, FluentValue> = args
            .iter()
            .map(|(k, v)| (Cow::Borrowed(*k), FluentValue::from(v.clone())))
            .collect();
        LOCALES
            .try_lookup_with_args(&self.lang, key, &map)
            .unwrap_or_else(|| key.to_string())
    }

    pub fn tn(&self, key: &str, name: &'static str, n: i64) -> String {
        let mut map: HashMap<Cow<'static, str>, FluentValue> = HashMap::new();
        map.insert(Cow::Borrowed(name), FluentValue::from(n));
        LOCALES
            .try_lookup_with_args(&self.lang, key, &map)
            .unwrap_or_else(|| key.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    fn keys(path: &str) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for entry in std::fs::read_dir(path).unwrap() {
            let text = std::fs::read_to_string(entry.unwrap().path()).unwrap();
            for line in text.lines() {
                if let Some((k, _)) = line.split_once(" =")
                    && !k.starts_with(' ')
                    && !k.starts_with('#')
                    && !k.is_empty()
                {
                    out.insert(k.trim().to_string());
                }
            }
        }
        out
    }

    /// Every key exists in both languages (§10).
    #[test]
    fn locales_complete() {
        let base = concat!(env!("CARGO_MANIFEST_DIR"), "/../../locales");
        let en = keys(&format!("{base}/en"));
        let ru = keys(&format!("{base}/ru"));
        let missing_ru: Vec<_> = en.difference(&ru).collect();
        let missing_en: Vec<_> = ru.difference(&en).collect();
        assert!(missing_ru.is_empty(), "missing in ru: {missing_ru:?}");
        assert!(missing_en.is_empty(), "missing in en: {missing_en:?}");
    }

    /// Every action has a localized label.
    #[test]
    fn actions_have_labels() {
        let base = concat!(env!("CARGO_MANIFEST_DIR"), "/../../locales/en");
        let en = keys(base);
        for a in at_core::keymap::Action::all() {
            let k = format!("action-{}", a.name().replace('_', "-"));
            assert!(en.contains(&k), "missing {k}");
        }
    }

    /// UI code uses keys, not hard-coded phrases (§4.14 lint).
    #[test]
    fn no_hardcoded_ui_strings() {
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
        let cyr = regex::Regex::new(r#""[^"\n]*[А-Яа-яЁё][^"\n]*""#).unwrap();
        for entry in std::fs::read_dir(src).unwrap() {
            let p = entry.unwrap().path();
            // This file holds the lint itself and expected translations.
            if p.file_name().and_then(|n| n.to_str()) == Some("i18n.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&p).unwrap();
            for (i, line) in text.lines().enumerate() {
                assert!(
                    !cyr.is_match(line),
                    "{}:{}: Cyrillic literal in code",
                    p.display(),
                    i + 1
                );
            }
        }
    }

    #[test]
    fn lookup_with_args() {
        let i = super::I18n::new("ru");
        assert_eq!(
            i.ta("reconnect-question", &[("host", "srv-01".into())]),
            "Переподключиться к srv-01?"
        );
        let e = super::I18n::new("en");
        assert_eq!(e.t("action-new-tab"), "New Tab (New Group)");
    }
}
