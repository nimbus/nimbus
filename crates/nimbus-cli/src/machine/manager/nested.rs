//! Host support for nested virtualization in the krunkit guest.
//!
//! The krun sandbox backend inside the machine guest needs `/dev/kvm`, which
//! the guest gets only when krunkit runs with `--nested`. Hypervisor.framework
//! offers EL2 to a guest only on Apple M3 or later with macOS 15 or later.
//! libkrun asks the framework directly (`hv_vm_config_get_el2_supported`), and
//! krunkit ignores `--nested` when that check fails, so krunkit stays the
//! authoritative gate. Nimbus mirrors the same rule conservatively from two
//! host facts before launch. It passes the flag only where it can work and
//! names the requirement where it cannot.

use nimbus::Error;

use super::emit_machine_info;

/// The host requirement for nested virtualization, as error text names it.
pub(super) const NESTED_VIRTUALIZATION_REQUIREMENT: &str =
    "Apple M3 or later and macOS 15 or later";

const MIN_APPLE_M_GENERATION: u32 = 3;
const MIN_MACOS_MAJOR: u32 = 15;

/// The host facts that decide nested virtualization support. A fact that the
/// host does not report is `None`, and an unknown fact disables nesting.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct NestedVirtualizationHost {
    /// `machdep.cpu.brand_string`, for example `Apple M3 Pro`.
    pub(super) cpu_brand: Option<String>,
    /// `kern.osproductversion`, for example `15.7.2`.
    pub(super) os_product_version: Option<String>,
}

impl NestedVirtualizationHost {
    /// Read the facts of the current host. A host other than macOS reports
    /// none.
    pub(super) fn current() -> Self {
        Self {
            cpu_brand: read_sysctl_string(c"machdep.cpu.brand_string"),
            os_product_version: read_sysctl_string(c"kern.osproductversion"),
        }
    }

    /// Whether krunkit can give the guest nested virtualization on this host.
    pub(super) fn supports_nested_virtualization(&self) -> bool {
        let chip_supported = self
            .cpu_brand
            .as_deref()
            .and_then(apple_m_generation)
            .is_some_and(|generation| generation >= MIN_APPLE_M_GENERATION);
        let os_supported = self
            .os_product_version
            .as_deref()
            .and_then(macos_major_version)
            .is_some_and(|major| major >= MIN_MACOS_MAJOR);
        chip_supported && os_supported
    }

    /// Fail when this host cannot give the guest nested virtualization. The
    /// error names the requirement and the facts that the host reported.
    pub(super) fn require_nested_virtualization(&self) -> Result<(), Error> {
        if self.supports_nested_virtualization() {
            return Ok(());
        }
        Err(Error::PreconditionFailed(format!(
            "nested virtualization in the machine guest requires \
             {NESTED_VIRTUALIZATION_REQUIREMENT}; this host reports CPU {} and macOS {}",
            self.cpu_brand.as_deref().unwrap_or("unknown"),
            self.os_product_version.as_deref().unwrap_or("unknown"),
        )))
    }

    /// Decide whether a krunkit launch gets `--nested`, and report the result
    /// to the operator.
    pub(super) fn krunkit_nested_virtualization(&self) -> bool {
        match self.require_nested_virtualization() {
            Ok(()) => {
                emit_machine_info("krunkit runs with --nested; the guest gets /dev/kvm");
                true
            }
            Err(reason) => {
                emit_machine_info(format!("krunkit runs without --nested: {reason}"));
                false
            }
        }
    }
}

/// The generation number of an Apple M-series brand string: `Apple M3 Max`
/// gives 3. Any other brand gives `None`.
fn apple_m_generation(cpu_brand: &str) -> Option<u32> {
    let rest = cpu_brand.trim().strip_prefix("Apple M")?;
    let digits = rest
        .find(|character: char| !character.is_ascii_digit())
        .map_or(rest, |end| &rest[..end]);
    digits.parse().ok()
}

/// The major version of a macOS product version: `15.7.2` gives 15.
fn macos_major_version(os_product_version: &str) -> Option<u32> {
    os_product_version.trim().split('.').next()?.parse().ok()
}

#[cfg(target_os = "macos")]
fn read_sysctl_string(name: &std::ffi::CStr) -> Option<String> {
    let mut len = 0usize;
    // SAFETY: a null output buffer asks sysctlbyname for the value size only.
    let status = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if status != 0 || len == 0 {
        return None;
    }
    let mut buffer = vec![0u8; len];
    // SAFETY: `buffer` is writable for `len` bytes, and sysctlbyname writes at
    // most `len` bytes and stores the written count back in `len`.
    let status = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            buffer.as_mut_ptr().cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if status != 0 {
        return None;
    }
    buffer.truncate(len);
    let value = std::ffi::CStr::from_bytes_until_nul(&buffer)
        .ok()?
        .to_str()
        .ok()?;
    Some(value.to_owned())
}

#[cfg(not(target_os = "macos"))]
fn read_sysctl_string(_name: &std::ffi::CStr) -> Option<String> {
    None
}
