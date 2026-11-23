#![allow(unused, clippy::all)]
mod part_1 {
    use hexa_analysis::domain::Language;
    use hexa_analysis::import_policy::{judge, Origin, Verdict};
}

mod part_2 {
    #[allow(unused_imports)]
    use hexa_analysis::domain::Language;
    use hexa_analysis::import_policy::{judge, Origin, Verdict};

    fn get_scaffolded_deny_entries() -> Vec<String> {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
        let possible_paths = [
            format!(
                "{}/../hexa-cli/assets/templates/ADR-rules.toml",
                manifest_dir
            ),
            format!("{}/assets/templates/ADR-rules.toml", manifest_dir),
            "../hexa-cli/assets/templates/ADR-rules.toml".to_string(),
            "assets/templates/ADR-rules.toml".to_string(),
        ];
        let content = possible_paths
            .iter()
            .find_map(|p| std::fs::read_to_string(p).ok())
            .expect("Failed to locate ADR-rules.toml template");

        let value: toml::Value = toml::from_str(&content).expect("Failed to parse ADR-rules.toml");

        let mut deny_list = Vec::new();
        find_deny_strings(&value, &mut deny_list);
        deny_list
    }

    fn find_deny_strings(val: &toml::Value, acc: &mut Vec<String>) {
        match val {
            toml::Value::Table(map) => {
                for (k, v) in map {
                    if k == "deny" {
                        if let toml::Value::Array(arr) = v {
                            for item in arr {
                                if let toml::Value::String(s) = item {
                                    acc.push(s.clone());
                                }
                            }
                        }
                    } else {
                        find_deny_strings(v, acc);
                    }
                }
            }
            toml::Value::Array(arr) => {
                for v in arr {
                    find_deny_strings(v, acc);
                }
            }
            _ => {}
        }
    }

    /// S14: A Go standard-library deny entry denies exactly the path equal to it; a module deny entry covers the packages below it
    #[test]
    fn s14_go_stdlib_deny_exact_and_module_deny_prefix() {
        let allow = vec![];
        let deny = vec!["net".to_string(), "github.com/jackc/pgx/v5".to_string()];

        // Standard library exact match denied
        assert_eq!(
            judge(Language::Go, "net", Origin::StandardLibrary, &allow, &deny),
            Verdict::DeniedByDeny
        );
        // Standard library below path NOT denied
        assert_ne!(
            judge(
                Language::Go,
                "net/netip",
                Origin::StandardLibrary,
                &allow,
                &deny
            ),
            Verdict::DeniedByDeny
        );

        // Module deny matches below path on / boundary
        assert_eq!(
            judge(
                Language::Go,
                "github.com/jackc/pgx/v5/pgxpool",
                Origin::External,
                &allow,
                &deny
            ),
            Verdict::DeniedByDeny
        );
    }
}

mod part_3 {
    use hexa_analysis::domain::Language;
    use hexa_analysis::import_policy::{judge, Origin, Verdict};

    /// S22: A Go module deny entry still covers the packages below it
    #[test]
    fn test_s22_go_module_deny_covers_subpackages() {
        let deny = vec!["github.com/jackc/pgx/v5".to_string()];
        let verdict = judge(
            Language::Go,
            "github.com/jackc/pgx/v5/pgxpool",
            Origin::External,
            &[],
            &deny,
        );
        assert_eq!(verdict, Verdict::DeniedByDeny);
    }
}
