use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};

use crate::{qemu_test, util::Target};

fn load_test_config_paths(target: Target, config: Option<PathBuf>) -> Result<Vec<PathBuf>> {
    if let Some(config_path) = config {
        let test_cfg = qemu_test::load_test_config(&config_path)?;
        if test_cfg.target.to_target() != target {
            return Err(anyhow!(
                "test config {} does not match --target",
                config_path.display()
            ));
        }
        return Ok(vec![config_path]);
    }

    let mut config_paths = glob::glob("test_cfgs/**/*_test.json")
        .context("failed to read test config glob pattern")?
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("failed while expanding test config paths")?;
    config_paths.sort();

    let mut filtered = Vec::new();
    for config_path in config_paths {
        let test_cfg = qemu_test::load_test_config(&config_path)?;
        if test_cfg.target.to_target() == target {
            filtered.push(config_path);
        }
    }

    Ok(filtered)
}

pub fn run_all(release: bool, target: Target, config: Option<PathBuf>) -> Result<()> {
    let mut total = 0_u32;
    let mut failed = 0_u32;

    for config_path in load_test_config_paths(target, config)? {
        total += 1;
        if let Err(err) = qemu_test::run(config_path.display().to_string(), release, false) {
            failed += 1;
            eprintln!("{}: {}", config_path.display(), err);
        }
    }

    if failed > 0 {
        return Err(anyhow!("{} out of {} tests failed", failed, total));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit_config() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../test_cfgs/unittest/x86-64_unittest_test.json")
    }

    #[test]
    fn selects_only_requested_config() {
        let config = unit_config();
        let paths = load_test_config_paths(Target::X86_64, Some(config.clone())).unwrap();
        assert_eq!(paths, vec![config]);
    }

    #[test]
    fn rejects_config_target_mismatch() {
        let err = load_test_config_paths(Target::Aarch64, Some(unit_config())).unwrap_err();
        assert!(err.to_string().contains("does not match --target"));
    }

    #[test]
    fn rejects_missing_config() {
        let config = unit_config().with_file_name("missing_test.json");
        assert!(load_test_config_paths(Target::X86_64, Some(config)).is_err());
    }
}
