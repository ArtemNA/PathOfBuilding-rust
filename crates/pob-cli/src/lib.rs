//! pob-cli library: the `calc` entry that produces the oracle candidate output.

use pob_golden::CandidateOutput;

/// Upstream branch reported as skipped while the engine is not ported (RUST-ARCH §5).
pub const ENGINE_UNIMPLEMENTED: &str = "Modules/Calcs.lua:384 calcs.buildOutput (engine not ported yet)";

/// Computes the build output. R0: no engine yet, so `stats` is empty and everything is `unimplemented`.
/// There is no XML parser at this stage, so only a 0-byte input is rejected.
pub fn calc(xml: &str) -> Result<CandidateOutput, String> {
    if xml.is_empty() {
        return Err("empty XML".into());
    }
    Ok(CandidateOutput {
        unimplemented: vec![ENGINE_UNIMPLEMENTED.into()],
        ..CandidateOutput::default()
    })
}
