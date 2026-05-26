use super::*;

fn machine_record_exists(roots: &MachineRootLayout, machine_name: &str) -> bool {
    roots.paths(machine_name).config_path.exists()
}

pub(in crate::machine) fn resolve_machine_ssh_target(
    command: &MachineSshCommand,
    roots: &MachineRootLayout,
) -> Result<(String, Vec<String>), Error> {
    let Some(first_arg) = command.args.first() else {
        return Ok((DEFAULT_MACHINE_NAME.to_owned(), Vec::new()));
    };

    if machine_record_exists(roots, first_arg) {
        return Ok((first_arg.clone(), command.args[1..].to_vec()));
    }

    Ok((DEFAULT_MACHINE_NAME.to_owned(), command.args.clone()))
}

pub(in crate::machine) fn resolve_machine_ssh_target_name<'a>(
    command: &'a MachineSshCommand,
    roots: &'a MachineRootLayout,
) -> Result<&'a str, Error> {
    if let Some(first_arg) = command.args.first()
        && machine_record_exists(roots, first_arg)
    {
        return Ok(first_arg.as_str());
    }

    Ok(DEFAULT_MACHINE_NAME)
}

pub(in crate::machine) fn resolve_machine_cp_target_name(
    command: &MachineCpCommand,
) -> Result<String, Error> {
    Ok(resolve_machine_cp_transfer(&command.src_path, &command.dest_path)?.machine_name)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::machine) enum MachineCpEndpoint {
    Host(String),
    Machine { name: String, path: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::machine) struct MachineCpTransfer {
    pub(in crate::machine) machine_name: String,
    pub(in crate::machine) machine_path: String,
    pub(in crate::machine) host_path: String,
    pub(in crate::machine) guest_is_src: bool,
}

pub(in crate::machine) fn resolve_machine_cp_transfer(
    src_path: &str,
    dest_path: &str,
) -> Result<MachineCpTransfer, Error> {
    let src = parse_machine_cp_endpoint(src_path)?;
    let dest = parse_machine_cp_endpoint(dest_path)?;

    match (src, dest) {
        (MachineCpEndpoint::Machine { name, path }, MachineCpEndpoint::Host(host_path)) => {
            Ok(MachineCpTransfer {
                machine_name: name,
                machine_path: path,
                host_path,
                guest_is_src: true,
            })
        }
        (MachineCpEndpoint::Host(host_path), MachineCpEndpoint::Machine { name, path }) => {
            Ok(MachineCpTransfer {
                machine_name: name,
                machine_path: path,
                host_path,
                guest_is_src: false,
            })
        }
        (MachineCpEndpoint::Machine { .. }, MachineCpEndpoint::Machine { .. }) => Err(
            Error::InvalidInput("copying between two machines is unsupported".to_owned()),
        ),
        (MachineCpEndpoint::Host(_), MachineCpEndpoint::Host(_)) => Err(Error::InvalidInput(
            "a machine name must prefix either the source path or destination path".to_owned(),
        )),
    }
}

pub(in crate::machine) fn parse_machine_cp_endpoint(
    value: &str,
) -> Result<MachineCpEndpoint, Error> {
    if looks_like_windows_host_path(value) {
        return Ok(MachineCpEndpoint::Host(value.to_owned()));
    }

    let Some((name, path)) = value.split_once(':') else {
        return Ok(MachineCpEndpoint::Host(value.to_owned()));
    };
    if name.is_empty() {
        return Ok(MachineCpEndpoint::Host(value.to_owned()));
    }
    if path.is_empty() {
        return Err(Error::InvalidInput(format!(
            "machine copy path '{}' is invalid; expected <machine>:<path>",
            value
        )));
    }

    Ok(MachineCpEndpoint::Machine {
        name: name.to_owned(),
        path: path.to_owned(),
    })
}

fn looks_like_windows_host_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/')
}
