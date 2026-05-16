// Some re-exported names are only consumed on `cfg(unix)` (or under
// `cfg(test)`) — on a Windows release build the consumers are
// stub-replaced and `-D unused-imports` would reject this list as
// unused. Keep the surface stable and silence the lint here rather
// than threading a cfg gate through every name.
#[allow(unused_imports)]
pub(super) use nimbus_machine::{
    MachineBootstrapMode, MachineConfigRecord, MachineGuestConfig, MachineGuestProvisioning,
    MachineHelperBinaryPaths, MachineImageFormat, MachineImageSource, MachineLifecycle,
    MachineManagerState, MachinePaths, MachineProvider, MachineResources, MachineRootLayout,
    MachineRuntimeState, MachineStateRecord, MachineVolume, resolve_runtime_root,
};
