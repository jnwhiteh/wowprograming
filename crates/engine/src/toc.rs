//! Parser for addon table of contents (`.toc`) files.
//!
//! The sandbox emulates the Retail game client, so load conditions are
//! evaluated for that client while parsing:
//!
//! - Path placeholders: `[Family]` is `Mainline` and `[Game]` is `Standard`.
//! - `[AllowLoadGameType a, b]` and `[ExcludeLoadGameType a b]` on file lines
//!   and metadata values; Retail matches the game types `mainline` and
//!   `standard`.
//! - `[AllowLoad glue]` lines are skipped (they belong to the login screen).
//! - `[Bootstrap]` marks files of a load-on-demand addon that run at startup.
//!
//! Other tags, such as `[AllowLoadEnvironment Global]`, are removed and
//! ignored.

/// Game types that the emulated client matches.
const GAME_TYPES: &[&str] = &["mainline", "standard"];
const FAMILY: &str = "Mainline";
const GAME: &str = "Standard";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TocFile {
    /// Path relative to the addon folder, with `/` separators.
    pub path: String,
    /// Runs at startup even when the addon is load on demand.
    pub bootstrap: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Toc {
    /// Metadata in file order, with keys as written.
    pub metadata: Vec<(String, String)>,
    /// Files to load in order. Files excluded by load conditions are left out.
    pub entries: Vec<TocFile>,
    /// Paths of all files in `entries`, for convenience.
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
                    let (value, tags) = split_tags(value);
                    if tags_allow(&tags) {
                        toc.metadata
                            .push((key.trim().to_string(), value.trim().to_string()));
                    }
                }
            } else if line.is_empty() || line.starts_with('#') {
                continue;
            } else {
                let (file, tags) = split_tags(line);
                if !tags_allow(&tags) {
                    continue;
                }
                let path = file
                    .replace("[Family]", FAMILY)
                    .replace("[Game]", GAME)
                    .replace('\\', "/");
                if path.is_empty() {
                    continue;
                }
                let bootstrap = tags.iter().any(|t| t.eq_ignore_ascii_case("Bootstrap"));
                toc.files.push(path.clone());
                toc.entries.push(TocFile { path, bootstrap });
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

    /// `LoadFirst`: the client loads these Blizzard addons before the others.
    pub fn load_first(&self) -> bool {
        self.get("LoadFirst").is_some_and(|v| v.trim() == "1")
    }

    pub fn load_on_demand(&self) -> bool {
        self.get("LoadOnDemand").is_some_and(|v| v.trim() == "1")
    }

    /// Whether the addon may load in the emulated game client, from its
    /// `AllowLoad` and `AllowLoadGameType` metadata.
    pub fn loadable_in_game(&self) -> bool {
        let environment_ok = self
            .get("AllowLoad")
            .is_none_or(|v| !v.trim().eq_ignore_ascii_case("glue"));
        let game_type_ok = self
            .get("AllowLoadGameType")
            .is_none_or(|v| words(v).any(|w| GAME_TYPES.iter().any(|g| g.eq_ignore_ascii_case(w))));
        environment_ok && game_type_ok
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

fn words(list: &str) -> impl Iterator<Item = &str> {
    list.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|w| !w.is_empty())
}

/// Split trailing `[...]` tags from a line. Placeholders such as
/// `[Family]` inside the path stay in place. Returns the rest of the line
/// and the tag contents in order.
fn split_tags(line: &str) -> (&str, Vec<String>) {
    let mut rest = line.trim_end();
    let mut tags = Vec::new();
    while rest.ends_with(']') {
        let Some(open) = rest.rfind('[') else { break };
        let tag = &rest[open + 1..rest.len() - 1];
        // A placeholder is part of a path, so it is followed by more path
        // characters; a trailing tag is preceded by whitespace or another tag.
        let before = rest[..open].trim_end();
        if before.len() == open && !before.is_empty() && !before.ends_with(']') {
            break;
        }
        tags.insert(0, tag.trim().to_string());
        rest = before;
    }
    (rest, tags)
}

/// Evaluate load-condition tags for the emulated client.
fn tags_allow(tags: &[String]) -> bool {
    tags.iter().all(|tag| {
        let (name, args) = tag.split_once(' ').unwrap_or((tag.as_str(), ""));
        let matches_game =
            || words(args).any(|w| GAME_TYPES.iter().any(|g| g.eq_ignore_ascii_case(w)));
        if name.eq_ignore_ascii_case("AllowLoadGameType") {
            matches_game()
        } else if name.eq_ignore_ascii_case("ExcludeLoadGameType") {
            !matches_game()
        } else if name.eq_ignore_ascii_case("AllowLoad") {
            !args.trim().eq_ignore_ascii_case("glue")
        } else {
            true
        }
    })
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
        assert!(toc.entries[2].bootstrap);
        assert!(!toc.entries[1].bootstrap);
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

    #[test]
    fn evaluates_load_conditions_for_retail() {
        let toc = Toc::parse(
            "## Dep: Blizzard_SharedXML\n\
             ## Dep: Blizzard_Plunderstorm [AllowLoadGameType plunderstorm]\n\
             [Family]\\Localization.lua [AllowLoadGameType mainline]\n\
             [Game]\\Localization.lua [AllowLoadGameType vanilla]\n\
             Classic\\Stubs.lua [AllowLoadGameType classic]\n\
             Shared.lua [ExcludeLoadGameType vanilla tbc]\n\
             NotVanilla.lua [ExcludeLoadGameType mainline]\n\
             Glue.lua [AllowLoad glue]\n\
             [Game]\\Popup.lua [AllowLoadGameType standard, plunderstorm]\n",
        );
        assert_eq!(toc.dependencies(), vec!["Blizzard_SharedXML"]);
        assert_eq!(
            toc.files,
            vec![
                "Mainline/Localization.lua",
                "Shared.lua",
                "Standard/Popup.lua"
            ]
        );
    }

    #[test]
    fn addon_level_conditions() {
        assert!(Toc::parse("## AllowLoad: Game\n").loadable_in_game());
        assert!(!Toc::parse("## AllowLoad: Glue\n").loadable_in_game());
        assert!(Toc::parse("## AllowLoadGameType: mainline\n").loadable_in_game());
        assert!(!Toc::parse("## AllowLoadGameType: classic\n").loadable_in_game());
        assert!(Toc::parse("## AllowLoadGameType: classic, standard\n").loadable_in_game());
    }
}
