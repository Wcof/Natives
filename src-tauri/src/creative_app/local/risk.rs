//! Command-risk classification for creative launch plans (P0).
//!
//! A command that can place real trades or move real funds must be blocked by
//! default; only an explicit, user-granted safe mode may run. The classifier is
//! generic — it inspects verbs and the *shape* of the command, never a product
//! name — so a future Compose runner inherits the same gate without hardcoding
//! any trading application.

use crate::creative_app::model::{LaunchPlan, LocalLaunchRuntime, ScriptRunner, TradeApproval};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandRisk {
    /// No real-funds signal; safe to auto-start.
    Safe,
    /// Trading-engine mode that is not real trading (webserver / dry-run / …);
    /// must still be gated behind explicit user choice where applicable.
    Warn,
    /// Can place real trades; auto-start is forbidden without explicit approval.
    Block,
}

/// Verbs that execute live/real trading. Bare `live` is intentionally excluded
/// (too ambiguous); the compound forms and `trade` are unambiguous.
const REAL_TRADING_VERBS: &[&str] = &[
    "trade",
    "live-trade",
    "live_trade",
    "start-trading",
    "real-trade",
    "spot-trade",
    "futures-trade",
    "margin-trade",
    "tradelive",
    "run-live",
];

/// Non-real-funds modes for a trading engine. These are gated but not blocked.
const SAFE_ENGINE_MODES: &[&str] = &[
    "webserver",
    "backtest",
    "backtesting",
    "download-data",
    "dry-run",
    "dry_run",
    "paper",
    "paper-trading",
    "show-config",
];

/// Benign project runners: their subcommand is a script name, not an executable,
/// so a `trade`-named script must NOT be classified from the package-manager arg.
fn is_benign_runner(program_bin: &str) -> bool {
    matches!(
        program_bin,
        "npm"
            | "pnpm"
            | "yarn"
            | "node"
            | "bun"
            | "deno"
            | "vite"
            | "vue-cli-service"
            | "next"
            | "nuxt"
    )
}

fn basename(program: &str) -> &str {
    program
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(program)
        .trim_end_matches(".exe")
        .trim_end_matches(".cmd")
}

fn is_real_trading_verb(s: &str) -> bool {
    REAL_TRADING_VERBS.contains(&s)
}

fn is_safe_engine_mode(s: &str) -> bool {
    SAFE_ENGINE_MODES.contains(&s)
}

/// Classify a resolved command (program + args). `program` may be empty when the
/// caller only has the command array; the first positional arg then stands in.
pub fn classify_command(program: &str, args: &[String]) -> CommandRisk {
    let program_bin = basename(program).to_ascii_lowercase();

    if is_real_trading_verb(&program_bin) {
        return CommandRisk::Block;
    }
    if is_benign_runner(&program_bin) {
        return CommandRisk::Safe;
    }

    // Engine invocation: <engine> trade ... / <engine> webserver ...
    if !program_bin.is_empty() {
        if let Some(first) = args.iter().find(|a| !a.starts_with('-')) {
            let v = first.to_ascii_lowercase();
            if is_safe_engine_mode(&v) {
                return CommandRisk::Warn;
            }
            if is_real_trading_verb(&v) {
                return CommandRisk::Block;
            }
        }
        return CommandRisk::Safe;
    }

    // Command-only array (no program): the first token is the verb.
    if let Some(first) = args.iter().find(|a| !a.starts_with('-')) {
        let v = first.to_ascii_lowercase();
        if is_real_trading_verb(&v) {
            return CommandRisk::Block;
        }
        if is_safe_engine_mode(&v) {
            return CommandRisk::Warn;
        }
    }
    CommandRisk::Safe
}

/// Classify the effective command a LaunchPlan would spawn.
pub fn plan_command_risk(plan: &LaunchPlan) -> CommandRisk {
    if plan.runtime == LocalLaunchRuntime::StaticHttp {
        return CommandRisk::Safe;
    }
    if plan.runtime == LocalLaunchRuntime::DockerCompose {
        if let Some(c) = &plan.compose {
            if !c.command.is_empty() {
                let risk = classify_command("", &c.command);
                if risk == CommandRisk::Block {
                    // An approval only relaxes the gate when the command actually
                    // matches the approved safe mode — a "webserver" approval can
                    // never run `trade`.
                    match plan.trade_approval {
                        Some(TradeApproval::Webserver)
                            if c.command.first().map(|t| t.as_str()) == Some("webserver") =>
                        {
                            return CommandRisk::Warn;
                        }
                        Some(TradeApproval::DryRun)
                            if c.command
                                .iter()
                                .any(|t| matches!(t.as_str(), "dry-run" | "dry_run" | "paper")) =>
                        {
                            return CommandRisk::Warn;
                        }
                        _ => return CommandRisk::Block,
                    }
                }
                return risk;
            }
            // No override: the compose default was already risk-checked at scan
            // time. The start preflight re-checks via the compose file.
            return CommandRisk::Warn;
        }
        return CommandRisk::Safe;
    }
    // Package-manager scripts were validated to a safe runner body (vite /
    // vue-cli / node); classify the resolved runner, not `npm run <name>`.
    if let Some(runner) = plan.script_runner {
        let program = match runner {
            ScriptRunner::Vite => "vite",
            ScriptRunner::VueCli => "vue-cli-service",
            ScriptRunner::Node => "node",
        };
        return classify_command(program, &[]);
    }
    let program = plan.program.as_str();
    let mut args: Vec<String> = plan.args.clone();
    if let Some(entry) = &plan.entry_file {
        if args.is_empty() {
            args.push(entry.clone());
        }
    }
    classify_command(program, &args)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn trade_verb_is_blocked() {
        assert_eq!(
            classify_command("freqtrade", &cmd(&["trade", "--config", "x.json"])),
            CommandRisk::Block
        );
        // Command-only array (image entrypoint-less compose).
        assert_eq!(
            classify_command("", &cmd(&["trade", "--config", "x.json"])),
            CommandRisk::Block
        );
        assert_eq!(
            classify_command("trade", &cmd(&["--config", "x"])),
            CommandRisk::Block
        );
        assert_eq!(
            classify_command("", &cmd(&["spot-trade", "--pair", "BTC/USDT"])),
            CommandRisk::Block
        );
    }

    #[test]
    fn safe_engine_modes_are_gated_not_blocked() {
        assert_eq!(
            classify_command("freqtrade", &cmd(&["webserver", "--config", "x.json"])),
            CommandRisk::Warn
        );
        assert_eq!(
            classify_command("freqtrade", &cmd(&["dry-run", "--config", "x.json"])),
            CommandRisk::Warn
        );
    }

    #[test]
    fn benign_project_runners_are_safe() {
        assert_eq!(
            classify_command("npm", &cmd(&["run", "trade"])),
            CommandRisk::Safe
        );
        assert_eq!(
            classify_command("node", &cmd(&["server.js"])),
            CommandRisk::Safe
        );
        assert_eq!(
            classify_command("vite", &cmd(&["--host", "127.0.0.1"])),
            CommandRisk::Safe
        );
    }

    #[test]
    fn unrelated_commands_are_safe() {
        assert_eq!(
            classify_command("", &cmd(&["serve", "--port", "3000"])),
            CommandRisk::Safe
        );
        assert_eq!(
            classify_command("", &cmd(&["backtest", "--data", "x"])),
            CommandRisk::Warn
        );
    }
}
