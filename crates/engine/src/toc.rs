//! Parser for addon table of contents (`.toc`) files.
//!
//! Only plain `<Name>.toc` files are supported. Trailing `[...]` tags on
//! file lines (such as `[AllowLoadEnvironment Global]`) are removed and not
//! evaluated. Path placeholders such as `[Family]` are not supported.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Toc {
    /// Metadata in file order, with keys as written.
    pub metadata: Vec<(String, String)>,
    /// Files to load, relative to the addon folder, with `/` separators.
    pub files: Vec<String>,
}

impl Toc {
    pub fn parse(text: &str) -> Toc {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let mut toc = Toc::default();
        for line in text.lines() {
            let line = line.trim();
            if let Some(meta) = line.strip_prefix("##") {
                if let Some((key, value)) = meta.split_once(':') {
                    toc.metadata
                        .push((key.trim().to_string(), value.trim().to_string()));
                }
            } else if line.is_empty() || line.starts_with('#') {
                continue;
            } else {
                let file = strip_tags(line).replace('\\', "/");
                if !file.is_empty() {
                    toc.files.push(file);
                }
            }
        }
        toc
    }

    /// Look up a metadata value. Keys are not case sensitive.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.metadata
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }

    pub fn title(&self) -> Option<&str> {
        self.get("Title")
    }

    /// Required dependencies: `Dependencies`, `RequiredDeps` and any key
    /// that starts with `Dep`.
    pub fn dependencies(&self) -> Vec<String> {
        self.list_where(|k| {
            let k = k.to_ascii_lowercase();
            k.starts_with("dep") || k.starts_with("requireddep")
        })
    }

    pub fn optional_dependencies(&self) -> Vec<String> {
        self.list_where(|k| k.to_ascii_lowercase().starts_with("optionaldep"))
    }

    pub fn load_on_demand(&self) -> bool {
        self.get("LoadOnDemand").is_some_and(|v| v.trim() == "1")
    }

    pub fn saved_variables(&self) -> Vec<String> {
        self.list_where(|k| k.eq_ignore_ascii_case("SavedVariables"))
    }

    pub fn saved_variables_per_character(&self) -> Vec<String> {
        self.list_where(|k| k.eq_ignore_ascii_case("SavedVariablesPerCharacter"))
    }

    fn list_where(&self, pred: impl Fn(&str) -> bool) -> Vec<String> {
        self.metadata
            .iter()
            .filter(|(k, _)| pred(k))
            .flat_map(|(_, v)| v.split(','))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }
}

/// Remove trailing `[...]` tags from a file line.
fn strip_tags(mut line: &str) -> &str {
    loop {
        line = line.trim_end();
        match (line.ends_with(']'), line.rfind('[')) {
            (true, Some(open)) => line = &line[..open],
            _ => return line,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\u{feff}## Interface: 120105\r\n\
        ## Title: |cff00ff00My Addon|r\r\n\
        ## Dependencies: LibA, LibB\r\n\
        ## Dep: LibC\r\n\
        ## RequiredDeps: LibD\r\n\
        ## OptionalDeps: Ace3\r\n\
        ## SavedVariables: MyDB, MyOtherDB\r\n\
        ## SavedVariablesPerCharacter: MyCharDB\r\n\
        ## X-Website: https://example.com\r\n\
        # a comment\r\n\
        \r\n\
        Libs\\LibStub\\LibStub.lua\r\n\
        Core.lua [AllowLoadEnvironment Global]\r\n\
        Frames.xml [AllowLoad game][Bootstrap]\r\n";

    #[test]
    fn parses_metadata_and_files() {
        let toc = Toc::parse(SAMPLE);
        assert_eq!(toc.get("interface"), Some("120105"));
        assert_eq!(toc.title(), Some("|cff00ff00My Addon|r"));
        assert_eq!(toc.get("X-Website"), Some("https://example.com"));
        assert_eq!(
            toc.files,
            vec!["Libs/LibStub/LibStub.lua", "Core.lua", "Frames.xml"]
        );
    }

    #[test]
    fn collects_dependency_lists() {
        let toc = Toc::parse(SAMPLE);
        assert_eq!(toc.dependencies(), vec!["LibA", "LibB", "LibC", "LibD"]);
        assert_eq!(toc.optional_dependencies(), vec!["Ace3"]);
        assert_eq!(toc.saved_variables(), vec!["MyDB", "MyOtherDB"]);
        assert_eq!(toc.saved_variables_per_character(), vec!["MyCharDB"]);
        assert!(!toc.load_on_demand());
    }

    #[test]
    fn reads_load_on_demand() {
        assert!(Toc::parse("## LoadOnDemand: 1\n").load_on_demand());
    }
}
