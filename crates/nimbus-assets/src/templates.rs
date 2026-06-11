pub mod convex {
    pub const SCHEMA_TS: &str = include_str!("../embedded/templates/convex/convex/schema.ts");
    pub const MESSAGES_TS: &str = include_str!("../embedded/templates/convex/convex/messages.ts");
    pub const GITIGNORE: &str = include_str!("../embedded/templates/convex/gitignore");
    pub const TSCONFIG_JSON: &str = include_str!("../embedded/templates/convex/tsconfig.json");
    pub const PACKAGE_JSON_TMPL: &str =
        include_str!("../embedded/templates/convex/package.json.tmpl");
}

pub mod cloud_functions {
    pub const FIREBASE_JSON: &str =
        include_str!("../embedded/templates/cloud-functions/firebase.json");
    pub const FUNCTIONS_PACKAGE_JSON_TMPL: &str =
        include_str!("../embedded/templates/cloud-functions/functions/package.json.tmpl");
    pub const FUNCTIONS_TSCONFIG_JSON: &str =
        include_str!("../embedded/templates/cloud-functions/functions/tsconfig.json");
    pub const FUNCTIONS_INDEX_TS: &str =
        include_str!("../embedded/templates/cloud-functions/functions/src/index.ts");
    pub const GITIGNORE: &str = include_str!("../embedded/templates/cloud-functions/gitignore");
}

pub mod machine {
    pub const READY_SERVICE: &str =
        include_str!("../embedded/templates/machine/ready.service.tmpl");
    pub const NIMBUS_SERVICE: &str =
        include_str!("../embedded/templates/machine/nimbus.service.tmpl");
    pub const NIMBUS_SOCKET: &str =
        include_str!("../embedded/templates/machine/nimbus.socket.tmpl");
    pub const VIRTIOFS_ROOT_OFF: &str =
        include_str!("../embedded/templates/machine/virtiofs-root-off.service");
    pub const VIRTIOFS_ROOT_ON: &str =
        include_str!("../embedded/templates/machine/virtiofs-root-on.service");
    pub const VIRTIOFS_MOUNT: &str =
        include_str!("../embedded/templates/machine/virtiofs-mount.service.tmpl");
}

#[cfg(test)]
mod tests {
    use super::{cloud_functions, convex, machine};

    #[test]
    fn init_templates_are_available() {
        assert!(convex::SCHEMA_TS.contains("defineSchema"));
        assert!(convex::PACKAGE_JSON_TMPL.contains("{{PROJECT_NAME}}"));
        assert!(cloud_functions::FIREBASE_JSON.contains("functions"));
        assert!(cloud_functions::FUNCTIONS_PACKAGE_JSON_TMPL.contains("{{PROJECT_NAME}}"));
    }

    #[test]
    fn machine_templates_are_available() {
        assert!(machine::READY_SERVICE.contains("{ready_vsock_port}"));
        assert!(machine::NIMBUS_SERVICE.contains("{guest_nimbus_bin}"));
        assert!(machine::NIMBUS_SOCKET.contains("{guest_nimbus_socket}"));
        assert!(machine::VIRTIOFS_MOUNT.contains("{target}"));
    }
}
