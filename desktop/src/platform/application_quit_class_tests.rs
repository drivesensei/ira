use super::*;
use std::ffi::CStr;

// Registered metadata lives for the process lifetime. No instances are allocated
// or initialized, and no selectors/global NSApplication are invoked by fixtures.
struct Classes {
    base: &'static AnyClass,
    companion: &'static AnyClass,
    wrong_parent: &'static AnyClass,
    unexpected: &'static AnyClass,
    grandchild: &'static AnyClass,
    missing: &'static AnyClass,
    inherited: &'static AnyClass,
    wrong_encoding: &'static AnyClass,
    shadow: &'static AnyClass,
    extra: &'static AnyClass,
}
fn register(name: &CStr, parent: &AnyClass, platform_kind: u8, extra: bool) -> &'static AnyClass {
    let mut builder = ClassBuilder::new(name, parent).expect("unique T101 fixture name");
    match platform_kind {
        0 => {}
        1 => builder.add_ivar::<*mut c_void>(c"platform"),
        2 => builder.add_ivar::<u64>(c"platform"),
        _ => unreachable!(),
    }
    if extra {
        builder.add_ivar::<u64>(c"t101_extra");
    }
    builder.register()
}
fn classes() -> &'static Classes {
    static CLASSES: OnceLock<Classes> = OnceLock::new();
    CLASSES.get_or_init(|| {
        let root = NSObject::class();
        let base = register(c"IRAT101MetadataBase", root, 1, false);
        let companion = register(c"IRAT101MetadataCompanion", base, 0, false);
        Classes {
            base,
            companion,
            wrong_parent: register(c"IRAT101MetadataWrongParent", companion, 0, false),
            unexpected: register(c"IRAT101MetadataUnexpected", base, 0, false),
            grandchild: register(c"IRAT101MetadataGrandchild", companion, 0, false),
            missing: register(c"IRAT101MetadataMissing", root, 0, false),
            inherited: register(c"IRAT101MetadataInheritedBase", base, 0, false),
            wrong_encoding: register(c"IRAT101MetadataWrongEncoding", root, 2, false),
            shadow: register(c"IRAT101MetadataShadow", base, 1, false),
            extra: register(c"IRAT101MetadataExtra", base, 0, true),
        }
    })
}
fn accepted(actual: &AnyClass, base: &AnyClass, companion: &CStr) -> bool {
    application_slot(actual, base, companion).is_ok()
}
#[test]
fn exact_base_returns_declaring_slot() {
    let c = classes();
    let slot = application_slot(c.base, c.base, c"IRAT101MetadataCompanion").unwrap();
    assert!(std::ptr::eq(
        slot,
        c.base.instance_variable(c"platform").unwrap()
    ));
}
#[test]
fn exact_direct_companion_returns_inherited_slot() {
    let c = classes();
    let slot = application_slot(c.companion, c.base, c"IRAT101MetadataCompanion").unwrap();
    assert!(std::ptr::eq(
        slot,
        c.base.instance_variable(c"platform").unwrap()
    ));
}
#[test]
fn exact_name_with_wrong_direct_parent_is_rejected() {
    let c = classes();
    // Has a usable platform slot, isolating the direct-parent guard.
    assert!(!accepted(
        c.wrong_parent,
        c.base,
        c"IRAT101MetadataWrongParent"
    ));
}
#[test]
fn unexpected_name_direct_child_is_rejected() {
    let c = classes();
    assert!(!accepted(c.unexpected, c.base, c"IRAT101MetadataCompanion"));
}
#[test]
fn allowed_name_grandchild_is_rejected() {
    let c = classes();
    assert!(!accepted(
        c.grandchild,
        c.base,
        c"IRAT101MetadataGrandchild"
    ));
}
#[test]
fn unrelated_root_is_rejected() {
    let c = classes();
    assert!(!accepted(
        NSObject::class(),
        c.base,
        c"IRAT101MetadataCompanion"
    ));
}
#[test]
fn missing_base_slot_is_rejected() {
    let c = classes();
    assert!(!accepted(c.missing, c.missing, c"IRAT101MetadataUnused"));
}
#[test]
fn inherited_base_slot_is_rejected() {
    let c = classes();
    assert!(!accepted(
        c.inherited,
        c.inherited,
        c"IRAT101MetadataUnused"
    ));
}
#[test]
fn wrong_base_encoding_is_rejected() {
    let c = classes();
    assert!(!accepted(
        c.wrong_encoding,
        c.wrong_encoding,
        c"IRAT101MetadataUnused"
    ));
}
#[test]
fn same_encoding_shadow_is_rejected() {
    let c = classes();
    assert_eq!(
        c.shadow
            .instance_variable(c"platform")
            .unwrap()
            .type_encoding(),
        c.base
            .instance_variable(c"platform")
            .unwrap()
            .type_encoding()
    );
    assert!(!std::ptr::eq(
        c.shadow.instance_variable(c"platform").unwrap(),
        c.base.instance_variable(c"platform").unwrap()
    ));
    assert!(!accepted(c.shadow, c.base, c"IRAT101MetadataShadow"));
}
#[test]
fn extra_nonshadowing_field_is_accepted() {
    let c = classes();
    assert!(c.extra.instance_size() > c.base.instance_size());
    assert!(accepted(c.extra, c.base, c"IRAT101MetadataExtra"));
}
#[test]
fn metaclass_is_rejected() {
    let c = classes();
    assert!(!accepted(
        c.base.metaclass(),
        c.base,
        c"IRAT101MetadataUnused"
    ));
}
#[test]
fn malformed_layout_is_rejected_without_ffi_metadata() {
    let size = std::mem::size_of::<*mut c_void>();
    assert!(platform_layout(0, size, size));
    assert!(platform_layout(size as isize, size * 2, size * 3));
    assert!(!platform_layout(-1, size, size));
    assert!(!platform_layout(1, size * 2, size * 2));
    assert!(!platform_layout(0, size - 1, size));
    assert!(!platform_layout(0, size, size - 1));
    assert!(!platform_layout(isize::MAX, usize::MAX, usize::MAX));
    // Checked arithmetic itself rejects overflow; the signed metadata offset
    // cannot represent a usize overflow on these pointer-width platforms.
    let aligned_overflow = usize::MAX - (std::mem::align_of::<*mut c_void>() - 1);
    assert!(!pointer_slot_fits(aligned_overflow, usize::MAX, usize::MAX));
}
#[test]
fn production_names_are_exact_literals() {
    assert_eq!(APPLICATION_CLASS, c"GPUIApplication");
    assert_eq!(APPLICATION_COMPANION, c"NSKVONotifying_GPUIApplication");
}
