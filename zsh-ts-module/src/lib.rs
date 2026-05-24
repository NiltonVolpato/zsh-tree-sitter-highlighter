use std::ffi::CStr;
use zsh_module::{state, builtin, Activate, Deactivate, Result, flags};

#[state]
#[derive(Debug, Default, Activate, Deactivate)]
struct HighlighterState;

#[builtin("zsh_ts_highlight", min = 0, max = 0, opts = "")]
fn zsh_ts_highlight(_state: &mut HighlighterState, _name: &CStr, _args: &[&CStr], _opts: &flags::Flags) -> Result<()> {
    println!("highlighting from module!");
    Ok(())
}
