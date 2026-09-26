//! What a run may do is decided by the claude command line built for its tier.

use std::ffi::OsStr;

use dispositif::config::Config;
use dispositif::runner::build_command;

fn cfg() -> Config {
    Config::parse(include_str!("../examples/config.toml")).unwrap()
}

fn args(cmd: &std::process::Command) -> Vec<String> {
    cmd.get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
}

fn has_pair(args: &[String], flag: &str, value: &str) -> bool {
    args.windows(2).any(|w| w[0] == flag && w[1] == value)
}

#[test]
fn restricted_tier() {
    let cfg = cfg();
    let a = args(&build_command(&cfg, &cfg.tiers["partner"], None));
    assert_eq!(&a[..3], ["-p", "--output-format", "json"]);
    assert!(a.contains(&"--restricted".into()));
    assert!(a.contains(&"--strict-mcp-config".into()));
    assert!(has_pair(&a, "--tools", "Read,Grep,Glob,WebFetch,WebSearch"));
    assert!(!a.contains(&"--permission-mode".into()));
    assert!(!a.contains(&"bypassPermissions".into()));
    assert!(!a.contains(&"--resume".into()));
}

#[test]
fn restricted_tier_never_bypasses_even_if_config_slipped() {
    let mut cfg = cfg();
    let tier = cfg.tiers.get_mut("partner").unwrap();
    tier.permission_mode = Some("bypassPermissions".into());
    let a = args(&build_command(&cfg, &cfg.tiers["partner"], None));
    assert!(!a.contains(&"bypassPermissions".into()), "{a:?}");
}

#[test]
fn full_tier() {
    let cfg = cfg();
    let a = args(&build_command(&cfg, &cfg.tiers["full"], None));
    assert!(has_pair(&a, "--permission-mode", "auto"));
    assert!(!a.contains(&"--restricted".into()));
    assert!(!a.contains(&"--strict-mcp-config".into()));
    assert!(!a.contains(&"--tools".into()));
}

#[test]
fn resume_only_when_given() {
    let cfg = cfg();
    let a = args(&build_command(&cfg, &cfg.tiers["full"], Some("sess-1")));
    assert!(has_pair(&a, "--resume", "sess-1"));
    let a = args(&build_command(&cfg, &cfg.tiers["partner"], Some("sess-2")));
    assert!(has_pair(&a, "--resume", "sess-2"));
}

#[test]
fn cwd_program_and_env() {
    let cfg = cfg();
    let cmd = build_command(&cfg, &cfg.tiers["partner"], None);
    assert_eq!(cmd.get_program(), OsStr::new("claude"));
    assert_eq!(
        cmd.get_current_dir().unwrap(),
        std::path::Path::new("/path/to/repo")
    );
    let home = std::env::var("HOME").unwrap();
    let env: Vec<_> = cmd.get_envs().collect();
    assert_eq!(
        env,
        [(
            OsStr::new("CLAUDE_CONFIG_DIR"),
            Some(OsStr::new(&format!("{home}/.claude")))
        )]
    );
}
