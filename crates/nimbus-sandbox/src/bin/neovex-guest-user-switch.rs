use std::env;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::Command;

const UID_ENV: &str = "NIMBUS_GUEST_UID";
const GID_ENV: &str = "NIMBUS_GUEST_GID";

fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(125);
        }
    }
}

fn run() -> Result<i32, String> {
    let uid = read_env_u32(UID_ENV)?;
    let gid = read_env_u32(GID_ENV)?;

    let mut argv = env::args_os();
    let _self = argv.next();
    let Some(program) = argv.next() else {
        return Err("guest user switch helper requires a target command".to_owned());
    };

    let status = Command::new(&program)
        .args(argv)
        .gid(gid)
        .uid(uid)
        .status()
        .map_err(|error| format!("failed to start guest command {:?}: {error}", program))?;

    if let Some(code) = status.code() {
        return Ok(code);
    }

    Ok(status.signal().map_or(1, |signal| 128 + signal))
}

fn read_env_u32(key: &str) -> Result<u32, String> {
    let raw =
        env::var(key).map_err(|_| format!("required environment variable {key} is missing"))?;
    raw.parse::<u32>()
        .map_err(|_| format!("environment variable {key} must be a u32, got {raw:?}"))
}

#[cfg(test)]
mod tests {
    use std::env;

    use super::read_env_u32;

    #[test]
    fn read_env_u32_parses_numeric_values() {
        unsafe {
            env::set_var("NIMBUS_TEST_NUMERIC_ENV", "42");
        }
        assert_eq!(
            read_env_u32("NIMBUS_TEST_NUMERIC_ENV").expect("env should parse"),
            42
        );
        unsafe {
            env::remove_var("NIMBUS_TEST_NUMERIC_ENV");
        }
    }

    #[test]
    fn read_env_u32_rejects_non_numeric_values() {
        unsafe {
            env::set_var("NIMBUS_TEST_INVALID_ENV", "abc");
        }
        let error = read_env_u32("NIMBUS_TEST_INVALID_ENV").expect_err("env should be rejected");
        assert!(
            error.contains("must be a u32"),
            "expected numeric parse error, got: {error}"
        );
        unsafe {
            env::remove_var("NIMBUS_TEST_INVALID_ENV");
        }
    }
}
