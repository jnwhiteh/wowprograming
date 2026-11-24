//! Coverage report: which script handlers in `UI.xsd` the engine fires,
//! and which documented widget methods it implements, with the number of
//! uses of each in Blizzard's Lua and XML.
//!
//! Methods are judged at run time from the widget metatables, not by
//! searching the source: a method is implemented, empty (marked with
//! `widgets::mark_empty`), a documented stub (built from the API
//! documentation), or missing.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use mlua::{Lua, Result, Table, Value};
use wowprogramming_lua::mlua;

use crate::apidoc::WIDGET_DOCS_KEY;
use crate::frames::FIRED_SCRIPTS;
use crate::state::state;
use crate::widgets::EMPTY_METHODS_KEY;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MethodState {
    Missing,
    Stub,
    Empty,
    Implemented,
}

impl MethodState {
    pub fn label(self) -> &'static str {
        match self {
            MethodState::Missing => "missing",
            MethodState::Stub => "stub",
            MethodState::Empty => "empty",
            MethodState::Implemented => "implemented",
        }
    }
}

/// Which goal an API belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Area {
    /// The frame runtime goal: generic widget behavior.
    Frames,
    /// The client state goal (tooltip data).
    Later,
    /// Models, game pads, movies, the browser, maps: out of scope for now.
    Out,
}

impl Area {
    pub fn label(self) -> &'static str {
        match self {
            Area::Frames => "frames",
            Area::Later => "later",
            Area::Out => "out",
        }
    }
}

#[derive(Debug, Clone)]
pub struct MethodRow {
    /// Documentation table, such as `SimpleFrameAPI`.
    pub doc: String,
    pub method: String,
    /// The worst state over the widget types that have this method.
    pub state: MethodState,
    pub uses: usize,
    pub area: Area,
}

#[derive(Debug, Clone)]
pub struct HandlerRow {
    pub name: String,
    pub fired: bool,
    pub uses: usize,
    pub area: Area,
}

#[derive(Debug, Default)]
pub struct Report {
    pub methods: Vec<MethodRow>,
    pub handlers: Vec<HandlerRow>,
}

/// Uses at which an unimplemented API counts as a gap for the goal.
pub const GAP_USES: usize = 5;

impl Report {
    /// Empty or missing methods and unfired handlers of the frame runtime
    /// with at least [`GAP_USES`] uses: what the goal must still do.
    pub fn gaps(&self) -> (Vec<&MethodRow>, Vec<&HandlerRow>) {
        let mut methods: Vec<&MethodRow> = self
            .methods
            .iter()
            .filter(|m| m.area == Area::Frames && m.uses >= GAP_USES)
            .filter(|m| matches!(m.state, MethodState::Empty | MethodState::Missing))
            .collect();
        methods.sort_by_key(|m| std::cmp::Reverse(m.uses));
        let mut handlers: Vec<&HandlerRow> = self
            .handlers
            .iter()
            .filter(|h| h.area == Area::Frames && !h.fired && h.uses >= GAP_USES)
            .collect();
        handlers.sort_by_key(|m| std::cmp::Reverse(m.uses));
        (methods, handlers)
    }

    /// The report as text. `all` lists every row; otherwise only what is
    /// not implemented, in the frame runtime's area, with uses.
    pub fn to_text(&self, all: bool) -> String {
        let mut out = String::new();
        let fired = self.handlers.iter().filter(|h| h.fired).count();
        out.push_str(&format!(
            "Script handlers in UI.xsd: {}, fired {}, not fired {}\n",
            self.handlers.len(),
            fired,
            self.handlers.len() - fired
        ));
        let count = |s: MethodState| self.methods.iter().filter(|m| m.state == s).count();
        out.push_str(&format!(
            "Documented widget methods: {}, implemented {}, empty {}, stub {}, missing {}\n",
            self.methods.len(),
            count(MethodState::Implemented),
            count(MethodState::Empty),
            count(MethodState::Stub),
            count(MethodState::Missing),
        ));
        let frames: Vec<&MethodRow> = self
            .methods
            .iter()
            .filter(|m| m.area == Area::Frames)
            .collect();
        let fcount = |s: MethodState| frames.iter().filter(|m| m.state == s).count();
        out.push_str(&format!(
            "  in the frame runtime's area: {}, implemented {}, empty {}, stub {}, missing {}\n",
            frames.len(),
            fcount(MethodState::Implemented),
            fcount(MethodState::Empty),
            fcount(MethodState::Stub),
            fcount(MethodState::Missing),
        ));
        let (gm, gh) = self.gaps();
        out.push_str(&format!(
            "\nGaps (frame runtime, {GAP_USES} or more uses, empty, missing or not fired): {}\n",
            gm.len() + gh.len()
        ));
        for h in &gh {
            out.push_str(&format!("  {:>5}  handler  {}\n", h.uses, h.name));
        }
        for m in &gm {
            out.push_str(&format!(
                "  {:>5}  {:<7}  {}:{}\n",
                m.uses,
                m.state.label(),
                m.doc,
                m.method
            ));
        }
        if all {
            out.push_str("\nAll handlers (fired, uses, area, name):\n");
            let mut hs: Vec<&HandlerRow> = self.handlers.iter().collect();
            hs.sort_by(|a, b| b.uses.cmp(&a.uses).then(a.name.cmp(&b.name)));
            for h in hs {
                out.push_str(&format!(
                    "  {:<9} {:>5}  {:<6} {}\n",
                    if h.fired { "fired" } else { "not fired" },
                    h.uses,
                    h.area.label(),
                    h.name
                ));
            }
            out.push_str("\nAll methods (state, uses, area, name):\n");
            let mut ms: Vec<&MethodRow> = self.methods.iter().collect();
            ms.sort_by(|a, b| a.doc.cmp(&b.doc).then(a.method.cmp(&b.method)));
            for m in ms {
                out.push_str(&format!(
                    "  {:<11} {:>5}  {:<6} {}:{}\n",
                    m.state.label(),
                    m.uses,
                    m.area.label(),
                    m.doc,
                    m.method
                ));
            }
        } else {
            let mut stubs: Vec<&MethodRow> = frames
                .iter()
                .copied()
                .filter(|m| m.state == MethodState::Stub && m.uses >= GAP_USES)
                .collect();
            stubs.sort_by_key(|m| std::cmp::Reverse(m.uses));
            out.push_str(&format!(
                "\nDocumented stubs (frame runtime, {GAP_USES} or more uses): {}\n",
                stubs.len()
            ));
            for m in stubs {
                out.push_str(&format!("  {:>5}  {}:{}\n", m.uses, m.doc, m.method));
            }
        }
        out
    }
}

/// Documentation tables: the widget types whose metatables hold their
/// methods, and the area.
const DOCS: &[(&str, &[&str], Area)] = &[
    (
        "SimpleObjectAPI",
        &["Frame", "Texture", "FontString"],
        Area::Frames,
    ),
    (
        "SimpleFrameScriptObjectAPI",
        &["Frame", "Texture", "FontString"],
        Area::Frames,
    ),
    (
        "SimpleScriptRegionAPI",
        &["Frame", "Texture", "FontString"],
        Area::Frames,
    ),
    (
        "SimpleScriptRegionResizingAPI",
        &["Frame", "Texture", "FontString"],
        Area::Frames,
    ),
    (
        "SimpleAnimatableObjectAPI",
        &["Frame", "Texture", "FontString"],
        Area::Frames,
    ),
    ("SimpleRegionAPI", &["Texture", "FontString"], Area::Frames),
    ("SimpleTextureBaseAPI", &["Texture"], Area::Frames),
    ("SimpleTextureAPI", &["Texture"], Area::Frames),
    ("SimpleMaskTextureAPI", &["MaskTexture"], Area::Frames),
    ("SimpleLineAPI", &["Line"], Area::Frames),
    ("SimpleFontAPI", &["Font"], Area::Frames),
    ("SimpleFontStringAPI", &["FontString"], Area::Frames),
    ("SimpleFrameAPI", &["Frame"], Area::Frames),
    ("SimpleButtonAPI", &["Button"], Area::Frames),
    ("SimpleCheckboxAPI", &["CheckButton"], Area::Frames),
    ("SimpleEditBoxAPI", &["EditBox"], Area::Frames),
    ("SimpleSliderAPI", &["Slider"], Area::Frames),
    ("SimpleStatusBarAPI", &["StatusBar"], Area::Frames),
    ("SimpleScrollFrameAPI", &["ScrollFrame"], Area::Frames),
    ("FrameAPICooldown", &["Cooldown"], Area::Frames),
    ("SimpleMessageFrameAPI", &["MessageFrame"], Area::Frames),
    ("SimpleHTMLAPI", &["SimpleHTML"], Area::Frames),
    ("SimpleColorSelectAPI", &["ColorSelect"], Area::Frames),
    ("SimpleAnimGroupAPI", &["AnimationGroup"], Area::Frames),
    ("SimpleAnimAPI", &["Animation"], Area::Frames),
    ("SimpleAnimAlphaAPI", &["Alpha"], Area::Frames),
    ("SimpleAnimFlipBookAPI", &["FlipBook"], Area::Frames),
    ("SimpleAnimPathAPI", &["Path"], Area::Frames),
    ("SimpleAnimRotationAPI", &["Rotation"], Area::Frames),
    ("SimpleAnimScaleAPI", &["Scale"], Area::Frames),
    ("SimpleAnimScaleLineAPI", &["LineScale"], Area::Frames),
    (
        "SimpleAnimTextureCoordTranslationAPI",
        &["TextureCoordTranslation"],
        Area::Frames,
    ),
    ("SimpleAnimTranslationAPI", &["Translation"], Area::Frames),
    (
        "SimpleAnimTranslationLineAPI",
        &["LineTranslation"],
        Area::Frames,
    ),
    ("SimpleAnimVertexColorAPI", &["VertexColor"], Area::Frames),
    ("FrameAPITooltip", &["GameTooltip"], Area::Later),
    ("SimpleModelAPI", &["Model"], Area::Out),
    ("SimpleModelFFXAPI", &["ModelFFX"], Area::Out),
    ("SimpleMapSceneAPI", &["MapScene"], Area::Out),
    ("FrameAPICharacterModelBase", &["PlayerModel"], Area::Out),
    ("FrameAPIDressUpModel", &["DressUpModel"], Area::Out),
    ("FrameAPICinematicModel", &["CinematicModel"], Area::Out),
    ("FrameAPITabardModelBase", &["TabardModel"], Area::Out),
    ("FrameAPITabardModel", &["TabardModel"], Area::Out),
    ("FrameAPIModelSceneFrame", &["ModelScene"], Area::Out),
    ("MinimapFrameAPI", &["Minimap"], Area::Out),
    ("SimpleMovieAPI", &["MovieFrame"], Area::Out),
    ("SimpleBrowserAPI", &["Browser"], Area::Out),
];

/// Handlers outside the frame runtime goal.
fn handler_area(name: &str) -> Area {
    const OUT: &[&str] = &[
        "OnUpdateModel",
        "OnModelCleared",
        "OnModelLoaded",
        "OnDressModel",
        "OnPanFinished",
        "OnGamePadButtonDown",
        "OnGamePadButtonUp",
        "OnGamePadStick",
        "OnMovieFinished",
        "OnMovieShowSubtitle",
        "OnMovieHideSubtitle",
        "OnExternalLink",
        "OnButtonUpdate",
        "OnError",
        "OnRequestNewSize",
        "OnUiMapChanged",
        "OnCharComposition",
        "OnInputLanguageChanged",
    ];
    if OUT.contains(&name) {
        Area::Out
    } else if name.starts_with("OnTooltipSet") || name == "OnTooltipAddMoney" {
        Area::Later
    } else {
        Area::Frames
    }
}

/// The handler names of `ScriptsType` in `UI.xsd`.
pub fn schema_handlers(xsd: &str) -> Vec<String> {
    let Some(start) = xsd.find("<xs:complexType name=\"ScriptsType\">") else {
        return Vec::new();
    };
    let body = &xsd[start..];
    let body = &body[..body.find("</xs:complexType>").unwrap_or(body.len())];
    body.match_indices("<xs:element name=\"")
        .filter_map(|(i, pat)| {
            let rest = &body[i + pat.len()..];
            rest.find('"').map(|e| rest[..e].to_string())
        })
        .collect()
}

/// Uses in Blizzard's Lua and XML: `:Name(` calls, and handler names
/// after `"` or `<` (SetScript, HookScript and XML script elements).
#[derive(Default)]
pub struct Uses {
    pub methods: HashMap<String, usize>,
    pub names: HashMap<String, usize>,
}

pub fn scan_uses(ui_source: &Path) -> Uses {
    let mut uses = Uses::default();
    let mut stack = vec![ui_source.join("Interface")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let path = e.path();
            if path.is_dir() {
                // The documentation names every method; it is not a use.
                if !path.ends_with("Blizzard_APIDocumentationGenerated") {
                    stack.push(path);
                }
                continue;
            }
            let ext = path
                .extension()
                .map(|x| x.to_string_lossy().to_ascii_lowercase());
            if !matches!(ext.as_deref(), Some("lua" | "xml")) {
                continue;
            }
            if let Ok(bytes) = std::fs::read(&path) {
                count_tokens(&bytes, &mut uses);
            }
        }
    }
    uses
}

fn count_tokens(bytes: &[u8], uses: &mut Uses) {
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if matches!(c, b':' | b'"' | b'<') {
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && ident(bytes[end]) {
                end += 1;
            }
            if end > start && bytes[start].is_ascii_alphabetic() {
                let word = String::from_utf8_lossy(&bytes[start..end]).into_owned();
                let next = bytes.get(end).copied();
                if c == b':' && next == Some(b'(') {
                    *uses.methods.entry(word).or_default() += 1;
                } else if c != b':' {
                    *uses.names.entry(word).or_default() += 1;
                }
                i = end;
                continue;
            }
        }
        i += 1;
    }
}

/// Build the report from the engine's widget metatables. Use counts come
/// from `uses` (see [`scan_uses`]); without it they are 0.
pub fn collect(lua: &Lua, xsd: &str, uses: &Uses) -> Result<Report> {
    let empty: HashSet<usize> = match lua.named_registry_value::<Value>(EMPTY_METHODS_KEY)? {
        Value::Table(t) => t
            .pairs::<Value, Value>()
            .flatten()
            .map(|(k, _)| k.to_pointer() as usize)
            .collect(),
        _ => HashSet::new(),
    };
    let docs: Table = match lua.named_registry_value::<Value>(WIDGET_DOCS_KEY)? {
        Value::Table(t) => t,
        _ => lua.create_table()?,
    };
    let mut stubs = HashSet::new();
    for (_, methods) in docs.pairs::<Value, Table>().flatten() {
        for (_, f) in methods.pairs::<Value, Value>().flatten() {
            stubs.insert(f.to_pointer() as usize);
        }
    }
    let indexes: HashMap<&str, Table> = {
        let st = state(lua);
        st.frame_metatables
            .iter()
            .filter_map(|(k, mt)| Some((*k, mt.raw_get::<Table>("__index").ok()?)))
            .collect()
    };

    let mut report = Report::default();
    for (doc, types, area) in DOCS {
        let Ok(methods) = docs.raw_get::<Table>(*doc) else {
            continue;
        };
        let mut names: Vec<String> = methods
            .pairs::<String, Value>()
            .flatten()
            .map(|(k, _)| k)
            .collect();
        names.sort();
        for name in names {
            let mut worst = MethodState::Implemented;
            for ty in *types {
                let Some(index) = indexes.get(ty) else {
                    worst = MethodState::Missing;
                    continue;
                };
                let v: Value = index.raw_get(name.as_str())?;
                let s = if v.is_nil() {
                    MethodState::Missing
                } else if empty.contains(&(v.to_pointer() as usize)) {
                    MethodState::Empty
                } else if stubs.contains(&(v.to_pointer() as usize)) {
                    MethodState::Stub
                } else {
                    MethodState::Implemented
                };
                worst = worst.min(s);
            }
            report.methods.push(MethodRow {
                doc: doc.to_string(),
                uses: uses.methods.get(&name).copied().unwrap_or(0),
                method: name,
                state: worst,
                area: *area,
            });
        }
    }
    for name in schema_handlers(xsd) {
        report.handlers.push(HandlerRow {
            fired: FIRED_SCRIPTS.contains(&name.as_str()),
            uses: uses.names.get(&name).copied().unwrap_or(0),
            area: handler_area(&name),
            name,
        });
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_calls_and_handler_names() {
        let mut uses = Uses::default();
        count_tokens(
            br#"f:SetScript("OnDragStart", fn) f:StartMoving() <OnDragStart method="X"/> a:b"#,
            &mut uses,
        );
        assert_eq!(uses.names.get("OnDragStart"), Some(&2));
        assert_eq!(uses.methods.get("StartMoving"), Some(&1));
        assert_eq!(uses.methods.get("SetScript"), Some(&1));
        assert_eq!(uses.methods.get("b"), None);
    }

    #[test]
    fn reads_scripts_type_from_the_schema() {
        let xsd = r#"<xs:complexType name="Other"><xs:element name="No"/></xs:complexType>
            <xs:complexType name="ScriptsType"><xs:choice>
            <xs:element name="OnLoad" type="ScriptType"/>
            <xs:element name="OnShow" type="ScriptType"/>
            </xs:choice></xs:complexType>"#;
        assert_eq!(schema_handlers(xsd), vec!["OnLoad", "OnShow"]);
    }
}
