use super::*;

#[test]
fn service_help_uses_shared_template_and_examples() {
    let error = RootCli::try_parse_from(["neovex", "service", "--help"])
        .expect_err("help should short-circuit");
    assert_eq!(error.kind(), clap::error::ErrorKind::DisplayHelp);
    let rendered = error.to_string();

    assert!(rendered.contains("Usage:"));
    assert!(rendered.contains("Available Commands:"));
    assert!(rendered.contains("Examples:"));
    assert!(rendered.contains("neovex service config"));
    assert!(rendered.contains("neovex service up"));
    assert!(rendered.contains("neovex service logs api --follow"));
    assert!(rendered.contains("Validate and print the resolved service plan"));
    assert!(rendered.contains("Start one or more declared services"));
    assert!(rendered.contains("Show persisted sandbox state"));
}

#[test]
fn service_leaf_help_uses_shared_template_and_examples() {
    let cases = [
        (
            vec!["neovex", "service", "config", "--help"],
            "neovex service config --services",
        ),
        (
            vec!["neovex", "service", "up", "--help"],
            "neovex service up api",
        ),
        (
            vec!["neovex", "service", "down", "--help"],
            "neovex service down api",
        ),
        (
            vec!["neovex", "service", "list", "--help"],
            "neovex service list --all-tenants",
        ),
        (
            vec!["neovex", "service", "inspect", "--help"],
            "neovex service inspect api",
        ),
        (
            vec!["neovex", "service", "logs", "--help"],
            "neovex service logs api --follow",
        ),
        (
            vec!["neovex", "service", "ps", "--help"],
            "neovex service ps api",
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
fn parses_service_config_subcommand() {
    let cli = RootCli::parse_from(["neovex", "service", "config", "--file", "stack.yml"]);
    let Some(RootCommand::Service(service)) = cli.command else {
        panic!("service subcommand should parse");
    };

    match service.command {
        ServiceSubcommand::Config(config) => {
            assert_eq!(config.file, PathBuf::from("stack.yml"));
            assert!(!config.services);
        }
        _ => panic!("expected config subcommand"),
    }
}

#[test]
fn parses_service_config_services_listing_flag() {
    let cli = RootCli::parse_from(["neovex", "service", "config", "--services"]);
    let Some(RootCommand::Service(service)) = cli.command else {
        panic!("service subcommand should parse");
    };

    match service.command {
        ServiceSubcommand::Config(config) => {
            assert_eq!(config.file, PathBuf::from(compose::DEFAULT_COMPOSE_FILE));
            assert!(config.services);
        }
        _ => panic!("expected config subcommand"),
    }
}

#[test]
fn parses_service_list_all_tenants_flag() {
    let cli = RootCli::parse_from(["neovex", "service", "list", "--all-tenants"]);
    let Some(RootCommand::Service(service)) = cli.command else {
        panic!("service subcommand should parse");
    };

    match service.command {
        ServiceSubcommand::List(list) => {
            assert_eq!(list.file, PathBuf::from(compose::DEFAULT_COMPOSE_FILE));
            assert!(list.all_tenants);
            assert_eq!(list.format, ServiceListOutputFormat::Table);
            assert!(!list.no_heading);
        }
        _ => panic!("expected list subcommand"),
    }
}

#[test]
fn parses_service_list_output_shaping_flags() {
    let cli = RootCli::parse_from(["neovex", "service", "list", "-f", "json", "--noheading"]);
    let Some(RootCommand::Service(service)) = cli.command else {
        panic!("service subcommand should parse");
    };

    match service.command {
        ServiceSubcommand::List(list) => {
            assert_eq!(list.format, ServiceListOutputFormat::Json);
            assert!(list.no_heading);
        }
        _ => panic!("expected list subcommand"),
    }
}

#[test]
fn parses_service_up_with_optional_service_and_tenant_override() {
    let cli = RootCli::parse_from(["neovex", "service", "up", "db", "--tenant", "svc-demo"]);
    let Some(RootCommand::Service(service)) = cli.command else {
        panic!("service subcommand should parse");
    };

    match service.command {
        ServiceSubcommand::Up(up) => {
            assert_eq!(up.service.as_deref(), Some("db"));
            assert_eq!(
                up.tenant.expect("tenant override should parse").as_str(),
                "svc-demo"
            );
            assert_eq!(up.file, PathBuf::from(compose::DEFAULT_COMPOSE_FILE));
        }
        _ => panic!("expected up subcommand"),
    }
}

#[test]
fn parses_service_down_without_service_uses_default_compose_file() {
    let cli = RootCli::parse_from(["neovex", "service", "down"]);
    let Some(RootCommand::Service(service)) = cli.command else {
        panic!("service subcommand should parse");
    };

    match service.command {
        ServiceSubcommand::Down(down) => {
            assert_eq!(down.service, None);
            assert_eq!(down.file, PathBuf::from(compose::DEFAULT_COMPOSE_FILE));
            assert_eq!(down.tenant, None);
        }
        _ => panic!("expected down subcommand"),
    }
}

#[test]
fn parses_service_inspect_with_optional_tenant_override() {
    let cli = RootCli::parse_from(["neovex", "service", "inspect", "db", "--tenant", "svc-demo"]);
    let Some(RootCommand::Service(service)) = cli.command else {
        panic!("service subcommand should parse");
    };

    match service.command {
        ServiceSubcommand::Inspect(inspect) => {
            assert_eq!(inspect.service, "db");
            assert_eq!(
                inspect
                    .tenant
                    .expect("tenant override should parse")
                    .as_str(),
                "svc-demo"
            );
            assert_eq!(inspect.format, ServiceInspectOutputFormat::Json);
        }
        _ => panic!("expected inspect subcommand"),
    }
}

#[test]
fn parses_service_inspect_format_flag() {
    let cli = RootCli::parse_from(["neovex", "service", "inspect", "db", "-f", "yaml"]);
    let Some(RootCommand::Service(service)) = cli.command else {
        panic!("service subcommand should parse");
    };

    match service.command {
        ServiceSubcommand::Inspect(inspect) => {
            assert_eq!(inspect.service, "db");
            assert_eq!(inspect.format, ServiceInspectOutputFormat::Yaml);
        }
        _ => panic!("expected inspect subcommand"),
    }
}

#[test]
fn parses_service_logs_with_follow_flag() {
    let cli = RootCli::parse_from(["neovex", "service", "logs", "db", "--follow"]);
    let Some(RootCommand::Service(service)) = cli.command else {
        panic!("service subcommand should parse");
    };

    match service.command {
        ServiceSubcommand::Logs(logs) => {
            assert_eq!(logs.service, "db");
            assert_eq!(logs.file, PathBuf::from(compose::DEFAULT_COMPOSE_FILE));
            assert!(logs.follow);
        }
        _ => panic!("expected logs subcommand"),
    }
}

#[test]
fn parses_service_ps_with_optional_tenant_override() {
    let cli = RootCli::parse_from(["neovex", "service", "ps", "db", "--tenant", "svc-demo"]);
    let Some(RootCommand::Service(service)) = cli.command else {
        panic!("service subcommand should parse");
    };

    match service.command {
        ServiceSubcommand::Ps(ps) => {
            assert_eq!(ps.service, "db");
            assert_eq!(
                ps.tenant.expect("tenant override should parse").as_str(),
                "svc-demo"
            );
            assert_eq!(ps.format, ServicePsOutputFormat::Table);
            assert!(!ps.no_heading);
        }
        _ => panic!("expected ps subcommand"),
    }
}

#[test]
fn parses_service_ps_output_shaping_flags() {
    let cli = RootCli::parse_from(["neovex", "service", "ps", "db", "-f", "json", "-n"]);
    let Some(RootCommand::Service(service)) = cli.command else {
        panic!("service subcommand should parse");
    };

    match service.command {
        ServiceSubcommand::Ps(ps) => {
            assert_eq!(ps.service, "db");
            assert_eq!(ps.format, ServicePsOutputFormat::Json);
            assert!(ps.no_heading);
        }
        _ => panic!("expected ps subcommand"),
    }
}

#[test]
fn service_list_help_describes_output_shaping_flags() {
    let error = RootCli::try_parse_from(["neovex", "service", "list", "--help"])
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
fn service_inspect_help_describes_short_format_flag() {
    let error = RootCli::try_parse_from(["neovex", "service", "inspect", "--help"])
        .expect_err("help should short-circuit");
    assert_eq!(error.kind(), clap::error::ErrorKind::DisplayHelp);
    let rendered = error.to_string();

    assert!(rendered.contains("--format"), "{rendered}");
    assert!(rendered.contains("-f"), "{rendered}");
    assert!(rendered.contains("json"), "{rendered}");
    assert!(rendered.contains("yaml"), "{rendered}");
}

#[test]
fn service_ps_help_describes_output_shaping_flags() {
    let error = RootCli::try_parse_from(["neovex", "service", "ps", "--help"])
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
