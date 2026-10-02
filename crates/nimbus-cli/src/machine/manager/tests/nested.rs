//! The host predicate for nested virtualization: Apple M3 or later and macOS 15
//! or later, with an unknown fact treated as unsupported.

use super::*;

fn host(cpu_brand: &str, os_product_version: &str) -> NestedVirtualizationHost {
    NestedVirtualizationHost {
        cpu_brand: Some(cpu_brand.to_owned()),
        os_product_version: Some(os_product_version.to_owned()),
    }
}

#[test]
fn m3_on_macos_15_supports_nested_virtualization() {
    assert!(host("Apple M3", "15.0").supports_nested_virtualization());
}

#[test]
fn later_chip_variants_and_macos_releases_support_nested_virtualization() {
    for (cpu_brand, os_product_version) in [
        ("Apple M3 Pro", "15.7.2"),
        ("Apple M3 Max", "15.1"),
        ("Apple M4", "26.0"),
        ("Apple M4 Ultra", "15.2"),
        ("Apple M10", "15.0"),
    ] {
        assert!(
            host(cpu_brand, os_product_version).supports_nested_virtualization(),
            "{cpu_brand} on macOS {os_product_version}"
        );
    }
}

#[test]
fn m2_on_macos_15_does_not_support_nested_virtualization() {
    assert!(!host("Apple M2 Max", "15.7.2").supports_nested_virtualization());
    assert!(!host("Apple M1", "26.0").supports_nested_virtualization());
}

#[test]
fn m3_on_macos_14_does_not_support_nested_virtualization() {
    assert!(!host("Apple M3", "14.7.1").supports_nested_virtualization());
}

#[test]
fn unknown_chip_does_not_support_nested_virtualization() {
    for cpu_brand in [
        "Intel(R) Core(TM) i9-9980HK CPU @ 2.40GHz",
        "Apple A18 Pro",
        "Apple M",
        "",
    ] {
        assert!(
            !host(cpu_brand, "15.0").supports_nested_virtualization(),
            "{cpu_brand:?}"
        );
    }
    let missing = NestedVirtualizationHost {
        cpu_brand: None,
        os_product_version: Some("15.0".to_owned()),
    };
    assert!(!missing.supports_nested_virtualization());
}

#[test]
fn unknown_macos_version_does_not_support_nested_virtualization() {
    assert!(!host("Apple M3", "unknown").supports_nested_virtualization());
    assert!(!NestedVirtualizationHost::default().supports_nested_virtualization());
}

#[test]
fn nested_virtualization_requirement_names_the_hardware_and_macos_floor() {
    let error = host("Apple M2 Max", "15.7.2")
        .require_nested_virtualization()
        .expect_err("an M2 host should not provide nested virtualization")
        .to_string();

    assert!(
        error.contains("Apple M3 or later and macOS 15 or later"),
        "{error}"
    );
    assert!(error.contains(NESTED_VIRTUALIZATION_REQUIREMENT), "{error}");
    assert!(error.contains("Apple M2 Max"), "{error}");
    assert!(error.contains("15.7.2"), "{error}");
}

#[cfg(target_os = "macos")]
#[test]
fn current_macos_host_reports_cpu_and_os_facts() {
    let current = NestedVirtualizationHost::current();

    let cpu_brand = current.cpu_brand.expect("macOS should report a CPU brand");
    assert!(!cpu_brand.is_empty());
    let os_product_version = current
        .os_product_version
        .expect("macOS should report a product version");
    let major = os_product_version
        .split('.')
        .next()
        .and_then(|major| major.parse::<u32>().ok());
    assert!(major.is_some(), "{os_product_version}");
}

#[test]
fn nested_virtualization_requirement_accepts_a_supported_host() {
    host("Apple M3 Pro", "15.0")
        .require_nested_virtualization()
        .expect("an M3 host on macOS 15 should provide nested virtualization");
}
