//! Macro options (`SecureCmdOptionParse`) and macro line execution,
//! without Blizzard's code. The rules are in docs/client-runtime.md.

use wowprogramming_engine::{Engine, EngineConfig};

fn started(wtf: &std::path::Path) -> Engine {
    let mut engine = Engine::new(EngineConfig {
        wtf_dir: wtf.to_path_buf(),
        blizzard_addons: Vec::new(),
        only_addons: Some(Vec::new()),
        ..EngineConfig::default()
    })
    .unwrap();
    engine.start();
    engine
}

/// The action and target of `SecureCmdOptionParse(msg)`, as "action@target".
fn parse(e: &Engine, msg: &str) -> String {
    e.eval(&format!(
        "local a, t = SecureCmdOptionParse({msg:?}) return tostring(a) .. '@' .. tostring(t)"
    ))
    .unwrap()
}

#[test]
fn options_pick_the_first_clause_that_holds() {
    let wtf = tempfile::tempdir().unwrap();
    let e = started(wtf.path());
    // No target: harm and exists fail, a clause without options holds.
    e.exec("UnitExists = function(u) return u == 'player' end UnitCanAttack = function() return false end");
    assert_eq!(parse(&e, "Frostbind"), "Frostbind@nil");
    assert_eq!(parse(&e, ""), "@nil");
    assert_eq!(parse(&e, "[harm] Frostbind; Aegis"), "Aegis@nil");
    assert_eq!(parse(&e, "[@player,exists] Aegis"), "Aegis@player");
    assert_eq!(parse(&e, "[target=player] Aegis"), "Aegis@player");
    assert_eq!(parse(&e, "[harm][@player] Aegis"), "Aegis@player");
    assert_eq!(parse(&e, "[nocombat,nomod] Rest"), "Rest@nil");
    assert_eq!(
        parse(&e, "[mod:shift] A; [button:2] B; [unknowncondition] C"),
        "nil@nil"
    );
    assert_eq!(parse(&e, "[nounknowncondition] C"), "C@nil");
}

#[test]
fn run_macro_text_sends_each_command_line() {
    let wtf = tempfile::tempdir().unwrap();
    let e = started(wtf.path());
    let lines: String = e
        .eval(
            "local got = {}
             C_Macro.SetMacroExecuteLineCallback(function(line) got[#got + 1] = line end)
             RunMacroText('#showtooltip X\\n/cast A\\r\\n\\n  /cast B  ')
             return table.concat(got, '|')",
        )
        .unwrap();
    assert_eq!(lines, "/cast A|/cast B");
}
