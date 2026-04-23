use super::*;

#[test]
fn compose_help_uses_shared_template_and_examples() {
    let error = RootCli::try_parse_from(["neovex", "compose", "--help"])
        .expect_err("help should short-circuit");
    assert_eq!(error.kind(), clap::error::ErrorKind::DisplayHelp);
    let rendered = error.to_string();

    assert!(rendered.contains("Usage:"));
    assert!(rendered.contains("Available Commands:"));
    assert!(rendered.contains("Examples:"));
    assert!(rendered.contains("neovex compose config"));
    assert!(rendered.contains("neovex compose up"));
    assert!(rendered.contains("neovex compose logs api --follow"));
    assert!(rendered.contains("Validate and print the resolved service plan"));
    assert!(rendered.contains("Start one or more declared services"));
    assert!(rendered.contains("Show persisted sandbox state"));
}

#[test]
fn compose_leaf_help_uses_shared_template_and_examples() {
    let cases = [
        (
            vec!["neovex", "compose", "config", "--help"],
            "neovex compose config --services",
        ),
        (
            vec!["neovex", "compose", "up", "--help"],
            "neovex compose up api",
        ),
        (
            vec!["neovex", "compose", "down", "--help"],
            "neovex compose down api",
        ),
        (
            vec!["neovex", "compose", "ps", "--help"],
            "neovex compose ps --all-tenants",
        ),
        (
            vec!["neovex", "compose", "inspect", "--help"],
            "neovex compose inspect api",
        ),
        (
            vec!["neovex", "compose", "logs", "--help"],
            "neovex compose logs api --follow",
        ),
        (
            vec!["neovex", "compose", "top", "--help"],
            "neovex compose top api",
        ),
    ];

    for (argv, example_snippet) in cases {
        let error = RootCli::try_parse_from(argv).expect_err("help should short-circuit");
        assert_eq!(error.kind(), clap::error::ErrorKind::DisplayHelp);
        let rendered = error.to_string();
        assert!(rendered.contains("Usage:"), "{rendered}");
        assert!(rendered.contains("Examples:"), "{rendered}");
        assert!(rendered.contains(example_snippet), "{rendered}");
    }
}

#[test]
fn legacy_service_namespace_is_not_supported() {
    let error = RootCli::try_parse_from(["neovex", "service", "up"])
        .expect_err("legacy service namespace should not parse");
    assert_eq!(error.kind(), clap::error::ErrorKind::InvalidSubcommand);
}

#[test]
fn parses_compose_config_subcommand() {
    let cli = RootCli::parse_from(["neovex", "compose", "config", "--file", "stack.yml"]);
    let Some(RootCommand::Compose(compose_command)) = cli.command else {
        panic!("compose subcommand should parse");
    };

    match compose_command.command {
        ComposeSubcommand::Config(config) => {
            assert_eq!(config.file, PathBuf::from("stack.yml"));
            assert!(!config.services);
        }
        _ => panic!("expected config subcommand"),
    }
}

#[test]
fn parses_compose_config_services_listing_flag() {
    let cli = RootCli::parse_from(["neovex", "compose", "config", "--services"]);
    let Some(RootCommand::Compose(compose_command)) = cli.command else {
        panic!("compose subcommand should parse");
    };

    match compose_command.command {
        ComposeSubcommand::Config(config) => {
            assert_eq!(config.file, PathBuf::from(file::DEFAULT_COMPOSE_FILE));
            assert!(config.services);
        }
        _ => panic!("expected config subcommand"),
    }
}

#[test]
fn parses_compose_ps_all_tenants_flag() {
    let cli = RootCli::parse_from(["neovex", "compose", "ps", "--all-tenants"]);
    let Some(RootCommand::Compose(compose_command)) = cli.command else {
        panic!("compose subcommand should parse");
    };

    match compose_command.command {
        ComposeSubcommand::Ps(list) => {
            assert_eq!(list.file, PathBuf::from(file::DEFAULT_COMPOSE_FILE));
            assert!(list.all_tenants);
            assert_eq!(list.format, ComposePsOutputFormat::Table);
            assert!(!list.no_heading);
        }
        _ => panic!("expected list subcommand"),
    }
}

#[test]
fn parses_compose_ps_output_shaping_flags() {
    let cli = RootCli::parse_from(["neovex", "compose", "ps", "-f", "json", "--noheading"]);
    let Some(RootCommand::Compose(compose_command)) = cli.command else {
        panic!("compose subcommand should parse");
    };

    match compose_command.command {
        ComposeSubcommand::Ps(list) => {
            assert_eq!(list.format, ComposePsOutputFormat::Json);
            assert!(list.no_heading);
        }
        _ => panic!("expected list subcommand"),
    }
}

#[test]
fn parses_compose_up_with_optional_service_and_tenant_override() {
    let cli = RootCli::parse_from(["neovex", "compose", "up", "db", "--tenant", "svc-demo"]);
    let Some(RootCommand::Compose(compose_command)) = cli.command else {
        panic!("compose subcommand should parse");
    };

    match compose_command.command {
        ComposeSubcommand::Up(up) => {
            assert_eq!(up.service.as_deref(), Some("db"));
            assert_eq!(
                up.tenant.expect("tenant override should parse").as_str(),
                "svc-demo"
            );
            assert_eq!(up.file, PathBuf::from(file::DEFAULT_COMPOSE_FILE));
        }
        _ => panic!("expected up subcommand"),
    }
}

#[test]
fn parses_compose_down_without_service_uses_default_compose_file() {
    let cli = RootCli::parse_from(["neovex", "compose", "down"]);
    let Some(RootCommand::Compose(compose_command)) = cli.command else {
        panic!("compose subcommand should parse");
    };

    match compose_command.command {
        ComposeSubcommand::Down(down) => {
            assert_eq!(down.service, None);
            assert_eq!(down.file, PathBuf::from(file::DEFAULT_COMPOSE_FILE));
            assert_eq!(down.tenant, None);
        }
        _ => panic!("expected down subcommand"),
    }
}

#[test]
fn parses_compose_inspect_with_optional_tenant_override() {
    let cli = RootCli::parse_from(["neovex", "compose", "inspect", "db", "--tenant", "svc-demo"]);
    let Some(RootCommand::Compose(compose_command)) = cli.command else {
        panic!("compose subcommand should parse");
    };

    match compose_command.command {
        ComposeSubcommand::Inspect(inspect) => {
            assert_eq!(inspect.service, "db");
            assert_eq!(
                inspect
                    .tenant
                    .expect("tenant override should parse")
                    .as_str(),
                "svc-demo"
            );
            assert_eq!(inspect.format, ComposeInspectOutputFormat::Json);
        }
        _ => panic!("expected inspect subcommand"),
    }
}

#[test]
fn parses_compose_inspect_format_flag() {
    let cli = RootCli::parse_from(["neovex", "compose", "inspect", "db", "-f", "yaml"]);
    let Some(RootCommand::Compose(compose_command)) = cli.command else {
        panic!("compose subcommand should parse");
    };

    match compose_command.command {
        ComposeSubcommand::Inspect(inspect) => {
            assert_eq!(inspect.service, "db");
            assert_eq!(inspect.format, ComposeInspectOutputFormat::Yaml);
        }
        _ => panic!("expected inspect subcommand"),
    }
}

#[test]
fn parses_compose_logs_with_follow_flag() {
    let cli = RootCli::parse_from(["neovex", "compose", "logs", "db", "--follow"]);
    let Some(RootCommand::Compose(compose_command)) = cli.command else {
        panic!("compose subcommand should parse");
    };

    match compose_command.command {
        ComposeSubcommand::Logs(logs) => {
            assert_eq!(logs.service, "db");
            assert_eq!(logs.file, PathBuf::from(file::DEFAULT_COMPOSE_FILE));
            assert!(logs.follow);
        }
        _ => panic!("expected logs subcommand"),
    }
}

#[test]
fn parses_compose_top_with_optional_tenant_override() {
    let cli = RootCli::parse_from(["neovex", "compose", "top", "db", "--tenant", "svc-demo"]);
    let Some(RootCommand::Compose(compose_command)) = cli.command else {
        panic!("compose subcommand should parse");
    };

    match compose_command.command {
        ComposeSubcommand::Top(ps) => {
            assert_eq!(ps.service, "db");
            assert_eq!(
                ps.tenant.expect("tenant override should parse").as_str(),
                "svc-demo"
            );
            assert_eq!(ps.format, ComposeTopOutputFormat::Table);
            assert!(!ps.no_heading);
        }
        _ => panic!("expected ps subcommand"),
    }
}

#[test]
fn parses_compose_top_output_shaping_flags() {
    let cli = RootCli::parse_from(["neovex", "compose", "top", "db", "-f", "json", "-n"]);
    let Some(RootCommand::Compose(compose_command)) = cli.command else {
        panic!("compose subcommand should parse");
    };

    match compose_command.command {
        ComposeSubcommand::Top(ps) => {
            assert_eq!(ps.service, "db");
            assert_eq!(ps.format, ComposeTopOutputFormat::Json);
            assert!(ps.no_heading);
        }
        _ => panic!("expected ps subcommand"),
    }
}

#[test]
fn compose_ps_help_describes_output_shaping_flags() {
    let error = RootCli::try_parse_from(["neovex", "compose", "ps", "--help"])
        .expect_err("help should short-circuit");
    assert_eq!(error.kind(), clap::error::ErrorKind::DisplayHelp);
    let rendered = error.to_string();

    assert!(rendered.contains("--format"), "{rendered}");
    assert!(rendered.contains("-f"), "{rendered}");
    assert!(rendered.contains("--noheading"), "{rendered}");
    assert!(rendered.contains("-n"), "{rendered}");
    assert!(rendered.contains("json"), "{rendered}");
    assert!(rendered.contains("yaml"), "{rendered}");
    assert!(rendered.contains("table"), "{rendered}");
}

#[test]
fn compose_inspect_help_describes_short_format_flag() {
    let error = RootCli::try_parse_from(["neovex", "compose", "inspect", "--help"])
        .expect_err("help should short-circuit");
    assert_eq!(error.kind(), clap::error::ErrorKind::DisplayHelp);
    let rendered = error.to_string();

    assert!(rendered.contains("--format"), "{rendered}");
    assert!(rendered.contains("-f"), "{rendered}");
    assert!(rendered.contains("json"), "{rendered}");
    assert!(rendered.contains("yaml"), "{rendered}");
}

#[test]
fn compose_top_help_describes_output_shaping_flags() {
    let error = RootCli::try_parse_from(["neovex", "compose", "top", "--help"])
        .expect_err("help should short-circuit");
    assert_eq!(error.kind(), clap::error::ErrorKind::DisplayHelp);
    let rendered = error.to_string();

    assert!(rendered.contains("--format"), "{rendered}");
    assert!(rendered.contains("-f"), "{rendered}");
    assert!(rendered.contains("--noheading"), "{rendered}");
    assert!(rendered.contains("-n"), "{rendered}");
    assert!(rendered.contains("json"), "{rendered}");
    assert!(rendered.contains("yaml"), "{rendered}");
    assert!(rendered.contains("table"), "{rendered}");
}
