//! `check` must refuse a config that could not be enforced as written.

use dispositif::config::Config;

const BASE: &str = r#"
agent_id = 1000001
agent_username = "example_agent"
preamble = "p"
fallback_reply = "f"

[[rule]]
name = "owner"
peer = "1000002"
from = [1000002]
trigger = "any"
trust = "full"

[tier.full]
cwd = "/tmp"
instructions = "i"
"#;

fn problems(toml: &str) -> String {
    format!(
        "{:#}",
        Config::parse(toml).expect_err("config should be rejected")
    )
}

#[test]
fn base_is_valid_with_defaults() {
    let cfg = Config::parse(BASE).unwrap();
    assert_eq!(cfg.mcp_url, "http://127.0.0.1:8788");
    assert_eq!(cfg.history, 15);
    assert_eq!(cfg.session_ttl_secs, 86_400);
    assert_eq!(cfg.claude_bin, "claude");
    assert_eq!(cfg.batch_quiet_secs, 5);
}

#[test]
fn batching_can_be_turned_off() {
    let cfg = Config::parse(&format!("batch_quiet_secs = 0\n{BASE}")).unwrap();
    assert_eq!(cfg.batch_quiet_secs, 0);
}

#[test]
fn example_config_is_valid() {
    Config::parse(include_str!("../examples/config.toml")).unwrap();
}

#[test]
fn rule_naming_a_missing_tier() {
    let e = problems(&BASE.replace(r#"trust = "full""#, r#"trust = "nope""#));
    assert!(e.contains(r#"trust "nope" names no [tier.nope]"#), "{e}");
}

#[test]
fn restricted_tier_without_tools() {
    let e = problems(&format!(
        "{BASE}\n[tier.r]\ncwd = \"/tmp\"\nrestricted = true\ninstructions = \"i\"\n"
    ));
    assert!(
        e.contains("restricted tier needs a non-empty tools list"),
        "{e}"
    );
    let e = problems(&format!(
        "{BASE}\n[tier.r]\ncwd = \"/tmp\"\nrestricted = true\ntools = \" \"\ninstructions = \"i\"\n"
    ));
    assert!(
        e.contains("restricted tier needs a non-empty tools list"),
        "{e}"
    );
}

#[test]
fn restricted_tier_cannot_bypass_permissions() {
    let e = problems(&format!(
        "{BASE}\n[tier.r]\ncwd = \"/tmp\"\nrestricted = true\ntools = \"Read\"\npermission_mode = \"bypassPermissions\"\ninstructions = \"i\"\n"
    ));
    assert!(
        e.contains("cannot use permission_mode bypassPermissions"),
        "{e}"
    );
}

fn restricted_with_tools(tools: &str) -> String {
    format!(
        "{BASE}\n[tier.r]\ncwd = \"/tmp\"\nrestricted = true\ntools = {tools:?}\ninstructions = \"i\"\n"
    )
}

#[test]
fn restricted_tier_cannot_list_a_shell() {
    let e = problems(&restricted_with_tools("Read, Bash"));
    assert!(e.contains(r#"not ["Bash"]"#), "{e}");
}

#[test]
fn restricted_tier_lists_only_read_only_tools() {
    // claude splits --tools on spaces too, so "Read Bash" would grant Bash;
    // Monitor runs commands; Write could plant a git hook.
    for bad in [
        "Read Bash",
        "Read,Monitor",
        "Read,Write",
        "Bash(ls:*)",
        "default",
        "read",
    ] {
        let e = problems(&restricted_with_tools(bad));
        assert!(e.contains("restricted tier may list only"), "{bad:?}: {e}");
    }
    Config::parse(&restricted_with_tools(
        " Read, Grep,Glob ,WebFetch,WebSearch",
    ))
    .unwrap();
}

#[test]
fn malformed_peer() {
    for bad in ["@someone", "", "12a", "-", "0", "* "] {
        let e = problems(&BASE.replace(r#"peer = "1000002""#, &format!("peer = {bad:?}")));
        assert!(
            e.contains("must be \"*\" or a numeric dialog id"),
            "{bad:?}: {e}"
        );
    }
    for good in ["*", "-1000003", "-1001234567890"] {
        Config::parse(&BASE.replace(r#"peer = "1000002""#, &format!("peer = {good:?}"))).unwrap();
    }
}

#[test]
fn malformed_from() {
    let e = problems(&BASE.replace("from = [1000002]", "from = []"));
    assert!(e.contains("from is empty"), "{e}");
    for bad in ["[0]", "[-1000003]", "[1000002, -5]"] {
        let e = problems(&BASE.replace("from = [1000002]", &format!("from = {bad}")));
        assert!(
            e.contains("from must hold bare user or channel ids"),
            "{bad}: {e}"
        );
    }
    let e = problems(&BASE.replace("from = [1000002]", r#"from = ["1000002"]"#));
    assert!(e.contains("from"), "{e}");
}

#[test]
fn unknown_trigger_and_fields_are_rejected() {
    problems(&BASE.replace(r#"trigger = "any""#, r#"trigger = "always""#));
    problems(&format!("{BASE}\nstray_key = 1\n"));
}

#[test]
fn username_with_at_sign() {
    let e = problems(&BASE.replace(r#""example_agent""#, r#""@example_agent""#));
    assert!(e.contains("bare username"), "{e}");
}

#[test]
fn all_problems_reported_at_once() {
    let bad = BASE
        .replace(r#"trust = "full""#, r#"trust = "nope""#)
        .replace(r#"peer = "1000002""#, r#"peer = "x""#);
    let e = problems(&bad);
    assert!(
        e.contains("names no [tier.nope]") && e.contains("numeric dialog id"),
        "{e}"
    );
}

#[test]
fn empty_fallback_reply() {
    let e = problems(&BASE.replace(r#"fallback_reply = "f""#, r#"fallback_reply = " \n""#));
    assert!(e.contains("fallback_reply is empty"), "{e}");
}

#[test]
fn unknown_permission_mode() {
    let e = problems(&BASE.replace("[tier.full]\n", "[tier.full]\npermission_mode = \"yolo\"\n"));
    assert!(e.contains(r#"permission_mode "yolo" is not one of"#), "{e}");
    for good in [
        "auto",
        "acceptEdits",
        "plan",
        "default",
        "dontAsk",
        "manual",
    ] {
        Config::parse(&BASE.replace(
            "[tier.full]\n",
            &format!("[tier.full]\npermission_mode = {good:?}\n"),
        ))
        .unwrap();
    }
}

#[test]
fn tools_without_restricted() {
    let e = problems(&BASE.replace("[tier.full]\n", "[tier.full]\ntools = \"Read\"\n"));
    assert!(
        e.contains("tools applies only with restricted = true"),
        "{e}"
    );
}

#[test]
fn mcp_url_must_be_plain_http() {
    let e = problems(&format!("mcp_url = \"https://example.com/mcp\"\n{BASE}"));
    assert!(e.contains("must be http://"), "{e}");
}

#[test]
fn channel_rules_and_dm_peer() {
    let channel = BASE.replace(
        r#"trigger = "any""#,
        "sender = \"channel\"\ntrigger = \"mention_or_reply\"",
    );
    let e = problems(&channel);
    assert!(e.contains("use trigger = \"any\""), "{e}");
    Config::parse(&BASE.replace(
        r#"trigger = "any""#,
        "sender = \"channel\"\ntrigger = \"any\"\nmin_chars = 300",
    ))
    .unwrap();
    for bad in ["*", "me", ""] {
        let e = problems(&BASE.replace(
            r#"instructions = "i""#,
            &format!("dm_peer = {bad:?}\ninstructions = \"i\""),
        ));
        assert!(e.contains("dm_peer"), "{bad}: {e}");
    }
    Config::parse(&BASE.replace(
        r#"instructions = "i""#,
        "dm_peer = \"1000002\"\ninstructions = \"i\"",
    ))
    .unwrap();
    problems(&BASE.replace(r#"trigger = "any""#, "sender = \"bot\"\ntrigger = \"any\""));
}
